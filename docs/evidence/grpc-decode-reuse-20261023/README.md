# Bounded stream decode reuse

Runtime and benchmark: `beb4ecd9a9b2143c440892607d5b486318062b61` (C11).
Matched baseline: `9d6fb9c416fc32732450fd78d32ee16ff8b802de` (C8).
Both use the same benchmark code, locks and Rust 1.99.1 toolchain.

C11 reuses decoded compressed-stream storage only when the codec fully consumes
its input and the original allocation remains uniquely owned. Owned prost
messages can return that storage; retained Bytes and lazy pbrs input keep their
ownership. This reduces selected allocation requests, with remaining byte and
instruction losses. Production readiness, complete specification coverage and
broad superiority over tonic/prost remain unproved.

## Ownership and limits

The private policy retains at most **4 MiB per stream buffer / 16 MiB process-wide**,
charged by full allocation capacity. A full cache or oversized backing declines
retention without failing the RPC. It does not preallocate the allowance.
Active decoding, application messages, compressor scratch and RSS are outside
this idle budget. Taking a buffer releases its idle lease; retaining it again
requires admission for its resulting actual capacity.

Logical initialized length is independent of cached physical capacity. A small
message does not initialize an entire previously large buffer. Cold short frames
keep the ordinary path. Output, history and member/frame limits, validation,
checksums and message ownership remain in force. Errors and observed terminal
states release cached storage; an unpolled live stream can retain its bounded
workspace until further polling or destruction.

## Validation

All results below retain their actual source and commands in the raw records.
Focused test counts overlap the workspace and are not added to its total.

| Check | Result |
|---|---|
| Clean all-feature workspace, C11 | **3,771 passed, zero failed, nine existing ignores**, 141 targets; 20 min 27.87 s |
| Ten pre-workspace checks | Passed, including strict lint/rustdoc and minimum Rust profiles |
| Focused library / wire / resolver | **703 / 1 / 34 passed** |
| Repaired Serving v2 / independent raw peer | **1,100 / 1 passed** |
| AddressSanitizer and leak detection | **746 passed, zero failed, one existing ignore** |
| Generic resource preview | Passed, 31.70 s; reused only after source/hash validation |
| Separate Prost-cache preview | Passed, **33.04 s / ten repeated cycles**; zero cached storage after each cycle, 560/560 acquired/released leases and FD recovery to the initial count of eight |
| Successor pipeline v2 | All eleven phases passed, including read-only adjudication, controls, captures and sanitizer |
| Capture/RPC accounting | **3,064 captures / 742,740 successful RPCs**, zero failures/timeouts |
| Actual C11 resource day | Started **2026-10-23 at 06:07 UTC**; running, result pending; pbrs workload does not exercise the new Prost cache path |
| Separate Prost-cache day | Started **2026-10-23 at 06:23 UTC**; running, result pending; no 24-hour qualification |

The workspace starts and ends at a clean C11 source. Sanitizer selection is not
an all-workspace sanitizer pass. Minimum-version, optional-feature, official
interop/compiler and platform results retain their stated profiles; local passes
do not certify every release gate. The generic resource preview/day does not
establish sustained retention behavior for the new prost cache path.

The separate cache harness uses a release all-feature Rust 1.99.1 build.
All 1,718 checked tracked files match `beb4ecd9`; only a new test overlay is
added. Its preview exited zero with verified effective process limits. Ten cycles
repeat the same fixture; they are not ten distinct tests. Each returns cached
capacity and buffer count to zero, with balanced leases and file descriptors.
The active day snapshot observes its first ten cycles in 33.6 seconds; this is
progress evidence, not a completed day or general production qualification.

## Allocation results

Selected source pairs: **568 captures / 21,300 RPCs**, two balanced ABBA repeats,
N25/50, 71 pair/workload scopes. Prost, gzip, plaintext, one connection/one in
flight in the following rows. Values are requested allocation bytes per RPC.

| Receiving side and workload | C8 | C11 | Change |
|---|---:|---:|---:|
| Server, 1 MiB bidi | 27.5 MiB | 18.5 MiB | -32.6% |
| Client, 1 MiB bidi | 28.5 MiB | 19.5 MiB | -31.5% |
| Server, 64 KiB upload | 1,516.3 KiB | 964.4 KiB | -36.4% |
| Client, 64 KiB server stream | 1,671.6 KiB | 1,119.8 KiB | -33.0% |

Current 1 MiB prost bidi bytes remain **18.2% above tonic on the server / 17.1%
on the client**. The 64 KiB upload server is 19.0% below its fresh tonic byte
control, and the server-stream client is 18.7% below. These are selected scopes.

The expanded current matrix separately passes **2,400 captures / 720,000 RPCs**:
all five RPC shapes, plaintext/TLS, identity/gzip, both native codecs, 1/64 KiB
and one/16 in-flight calls; three repeats and N200/400. At 1 KiB identity bidi,
prost bytes remain **36.7% above tonic on the client / 31.9% on the server**.
Those medians must not be substituted into the smaller selected source pairs.

The full pbrs interop server requests **43.5 MiB versus 15.7 MiB (+178%)** for
selected 1 MiB gzip bidi. Expanded 1 KiB identity bidi server bytes are 97.8% higher.
This route includes extra interop handler work; it is not a codec-only or isolated
kernel comparison. Retaining input is valid behavior and is not disabled to force
reuse. The raw ledgers keep every codec and both endpoint directions.

All **112** native allocation/byte source groups remain recorded: **45** have a
higher median and **64** contain a higher-cost repeat, without a 2% cutoff.
These group counts are not significance tests. Largest selected median increase:
1 MiB gzip server-stream server bytes **+2.88%**, with mixed repeat directions.
Tiny identity bidi client allocations rise 1.6% in both repeats. Tiny unary client
bytes rise 0.857%, while the fresh tonic source control rises 0.866%; no allocation-
site cause was established. Repeat ranges and paired ratios are retained.

Requested bytes are cumulative allocator requests, not retained memory or RSS.
Percentages here use ratios of unrounded medians. Shared host, zero-filled
application payloads, gzip level 6 and four messages on each streaming leg.
Server send buffers match at 400 KiB and request queues at 16. Shipping defaults,
validation and production native accounting/channelz remain enabled.

## Instruction diagnostics

**96 captures / 1,440 RPCs**, two balanced ABBA repeats, N10/20, prost gzip,
plaintext and one in flight. Both endpoints run under Callgrind with allocation-
counting release binaries. Native clients and servers are measured in this scope.

| Native side and workload | Million Ir/RPC | Tonic/prost | Difference |
|---|---:|---:|---:|
| Client, 1 KiB upload | 2.385 | 2.237 | +6.6% |
| Server, 1 KiB bidi | 2.834 | 2.774 | +2.2% |
| Server, 1 KiB upload | 1.063 | 1.163 | -8.6% |
| Server, 64 KiB upload | 1.939 | 2.037 | -4.8% |

The other four cases are within the 2% tonic tie band. **All eight C8→C11 source
changes are under 0.4%; no C11 CPU gain is established.** These are instrumented
small-N diagnostics, not shipping CPU, native latency or throughput qualification.

Read-only attribution of the retained client upload profiles conserves every
exclusive instruction total and incoming fill edge. The native 1 KiB gzip upload
cost is **+6.629%**; repeated writer output-scratch initialization is the main
extra work. Common compressor initialization is equal. The raw review and JSON
(`architecture-c11-client-upload-ir-review.md/json`) retain profiles, disassembly,
function/edge sums and source versions. No replacement has been implemented or
measured. Coarse process CPU ticks and context switches are not speed rankings;
syscalls and task wakes remain unmeasured.

## Failed attempts and control interpretation

- The initial C10 build selected Rust 1.88 and was rejected before comparison;
  the corrected stable build is a separate record. C10 had no measured campaign.
- C10 `a675b61f` workspace failed: **3,766 passed, two failed, nine ignored**,
  140 targets. Resolver phase expiry and an unadmitted-child fixture precondition
  were repaired with explicit causal witnesses, without widening statuses or timeouts.
- The first C11 Serving gate retained **1,099 passes / one mTLS startup failure**;
  its raw peer result and exact repaired v2 gate remain separate records.
- The original C11 pipeline failed its automated unit marker classifier: it
  reported three of four controls detected. The uniqueness assertion actually
  rejected the corrupted backing before the later capacity marker; the other
  three controls detected their faults. Independent read-only adjudication
  passed against the original source, compilation and raw failures, with **no
  new tests**. The original controller remains failed; it was not relabeled.
- After restoration, 703 library and one wire check passed. Pipeline v2 is a
  separate successor; its completed validation reuses source-attested preview
  and adjudication evidence explicitly. Earlier failures stay archived too.

## Remaining work and raw records

Source review identifies a current pre-shape custom-Service deadline origin and
outer-owner gap. It exists without a new task queue. The independent raw-peer
reproduction is **NOT RUN**, and no repair is claimed. Fixed admission deadlines,
one valid terminal responder and actual cleanup are the next correctness work.

Other proposals remain unimplemented: profile-guided gzip output handling,
known-single frame reservations, separately tuned scratch vectors, bounded encode
storage, shared-input pending ownership, checked owned read-all decode and task
isolation. Their reviews and unapplied patches are not performance results.
Complete speed, interop, feature/platform/release and actual sustained cache/day
qualification remain open; C8's separate day cannot qualify C11.

[`results.json`](results.json) retains full scopes and counters.
[`manifest.json`](manifest.json) indexes hashes and archive members in
[`raw.tar.gz`](raw.tar.gz); `check.py` verifies the archived files. Source files,
commands, raw failures, reviews and control mutations are retained. Executables
are represented by hashes. This capsule does not rewrite older evidence.

The frozen `results.json` predates the separate cache-day launch and contains the
earlier two-day snapshot, not all three active runs. Later cache proof is retained
separately in raw `architecture-c11-prost-cache-preview-evidence/` and
`architecture-c11-prost-cache-day-progress-snapshot.json`. The once-only snapshot
is authoritative for its observed progress; it does not predict a terminal result.
