# Atomic admission: isolated h2 component evidence

The copied h2 prototype passes **18 component tests**, including cancellation
before admission, dynamic peer limits, queued legacy requests, cancellation
after admission, flow-controlled upload, held response bodies, GOAWAY, and EOF.
The unchanged mixed-API cancellation test first fails against the earlier
seven-pass library: its old `poll_ready` path retains the new task's waker on an
older queued stream. Removing that path only from the additive API fixes this
specific component defect.

This is an isolated backend investigation. **Shipping source, Cargo manifests,
dependency pins, and the original retry fixture remain unchanged.** The original
all-feature failure and both [raw RPC deadline regressions](../raw-peer-two-red.log)
remain red. These component results do not close CL-07a or qualify a native
caller integration, four RPC shapes, early-terminal upload, gzip, Tower body
opens, TLS/reconnect generations, packaging, or performance.

## Distinct retained stages

| Snapshot | Actual result | Linked library SHA256 prefix | Retained evidence |
|---|---|---|---|
| Check-only, no pending-queue reservation | 1 passed / 6 failed | `0f4c11278def` | [first failed log](check-only-rejected/first-tests.log), exact three-file patch, original source pins, complete reconstructed source archive |
| Queue reservation, transient assertion and early outer wake | Compiled; **not tested** | `debad05c95b8` | [original pins](queue-reserved-untested/source-pins.json), original five-file archive, complete reconstructed backend archive, empty compiler stderr |
| Queue reservation, locked assertion and post-transition outer wake; still calls old readiness | 7 passed / 0 failed, exit 0 | `4136a19e90d8` | [raw result](queue-reserved-corrected-seven/test.stdout), original build/test argv and pins, complete source archive |
| Same seven-pass backend, unchanged newer composed harness | **0 passed / 1 failed**, exit 101 | `4136a19e90d8` | [specific waker failure](mixed-waker-red/test.stderr), [guarded argv/pins](mixed-waker-red/pins.json), complete old-backend/new-harness archive |
| Additive-only readiness, original APIs intact | **18 passed / 0 failed**, exit 0 | `bdf32eeb714d` | [raw result](additive-only-eighteen/test.stdout), [build/test argv and pins](additive-only-eighteen/pins.json), complete source archive and readable harnesses |

The seven- and eighteen-test captures used an OS `timeout 45` bound. The new
specific red also used that bound and selected only
`additional::mixed_legacy_pending_cancel_releases_only_new_task_and_request`.
Its executable, compiler, earlier linked library, eleven extern artifacts,
unchanged harness files, source archives, and the coordinator's current source
inventory were checked before and after the run; no drift occurred. The
coordinator's newer source inventory is recorded separately and was **not** the
backend linked by that red.

Original test-build and test-launch argv were not retained for the first
check-only run; its unchanged raw compiler/test logs, library-build argv,
changed-file hashes, original harness, and workspace executable/library hashes
are retained. That provenance gap is not filled with invented launch metadata.
The untested stage remains untested. Its complete archive is a packaging
reconstruction from the retained five exact files and verified original source;
no new compile or test was performed for it.

## What the prototype changes

The [five-file patch](backend.patch) adds an owned request future and an atomic
check-and-enqueue operation under h2's existing `Streams` mutex. While capacity
is full, the request remains untouched and has no stream ID or queued HEADERS.
The authoritative count includes active streams **and actual `pending_open`
queue entries**. Insertion increments pending count once; writer pop transfers
pending quota to active quota under the same mutex; queue clearing decrements
pending count. Real active release still requires protocol closure and empty
outbound frame/data queues. Shrinking SETTINGS can leave existing reservations
above the new cap; it prevents further admission until capacity becomes free.

The [final readiness correction](additive-only-readiness.patch) removes old
per-Sender readiness and its pending rejection only from the new API. Locked
connection-error and stream-ID checks still precede capacity registration and
request consumption. Original `SendRequest::send_request`, `poll_ready`, inner
`Streams::send_request`, and reset handling remain byte-identical to upstream;
their function hashes are in the [packaging audit](packaging-audit.json).
Sender's existing pending field remains intact. Preserving pending-open state
keeps the upstream rule that HEADERS must precede a reset on a previously idle
stream.

Capacity waiters use weak registrations owned by the new future. The capacity
predicate and registration share the mutex, closing the ordinary lost-wake
window. Broadcast wakeups provide **no FIFO or bounded-starvation guarantee**.
Native's initial limit is zero; upstream Builder defaults to unlimited before
first SETTINGS. The additional tests preserve both policies and distinguish
omitted MAX in initial versus later SETTINGS.

The [additional harness](additive-only-eighteen/admission_additional_tests.rs)
retains its original uncompiled-draft header because its exact bytes were frozen
before the recorded eighteen-test compilation; the raw result records that
later successful compilation and execution. The harness
fences polling the entire connection before testing admitted cancellation;
blocking IO writes alone would not certify that state. Its parser retains
partial bytes across canceled quiet reads, and its SETTINGS ACK helper retains
preceding stream frames. Explicit cancel and last-handle drop both require
HEADERS then CANCEL RST; terminal GOAWAY accepts clean EOF while rejecting any
stream frame or partial frame before EOF. A three-byte upload with one byte of
stream credit proves that response EOF does not release quota while request
DATA is still queued. A separate body fixture proves protocol EOF can release
quota while terminal user handles and unread response bytes remain alive.

## Provenance and replay

The source is registry **h2 0.4.19**, upstream VCS
`d57d1b852fec9dda6d42d3454502006d52104da8`, archive checksum
`ef8e5e5a340588f4452631496976cf8636d4a7ecf600239fdc27615d2530bc16`.
All **68 original source-file checksums** and the unchanged registry lock pin
are retained in [upstream provenance](upstream-provenance.json), with the exact
[MIT license](upstream.LICENSE), [VCS metadata](upstream-vcs.json), and complete
original source archive. Packaging verified the original cache and archive
without modifying them. This is a minimal upstream-derived patch investigation,
not a custom HTTP/2 engine or an approved dependency topology.

Each stage's `source.tar.gz` contains the complete backend source and every
harness used for that stage. The untested stage contains only its backend;
the specific waker red contains the earlier backend and unchanged newer
two-file harness. The complete check-only source was reconstructed from its
retained patch, then checked against all three original changed-file hashes.
The original partial archive is also retained for the untested stage. Archive
checksums and all raw log/pin hashes are in [the inventory](inventory.json).

Executable ELF files and rlibs remain workspace-only;
[their full hashes and paths](workspace-binaries.json) are committed. No binary
ELF/rlib is added to Git. The exact Rust 1.99 compiler path, hash, verbose
version, dependency artifact hashes, and compiler argv are recorded in the
specific red's pins. Other source dependencies/toolchain binaries are not
vendored into this evidence pack.

To replay, unpack a stage's complete source archive into a new scratch
directory and use its recorded compiler argv, changing only extracted source
and output paths and supplying the exact pinned extern artifacts. The newer
harness expects `admission_additional_tests.rs` alongside `admission_tests.rs`.
For the specific red, use the old seven-pass backend/library with that newer
harness and retain the exact test selector and timeout. This relocates the
captured command; it does not make a different dependency graph equivalent.

No native caller integration or shipping graph change is approved by this
pack. A root-only Cargo patch does not propagate to independent path consumers.
A future topology must also preserve error/status behavior: current seam
`Error::source()` forwards upstream's empty source, so existing boxed seam
errors normally produce tonic UNKNOWN. Direct registry `h2::Error` has a
different tonic conversion path. Those two oracles must stay distinct; applying
the raw reason table to the seam would change existing behavior.
