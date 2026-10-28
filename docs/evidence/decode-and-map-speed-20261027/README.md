# Codec, generator and transport gains, October 27

Implementation `71422f00` reuses validated child wire during lazy access,
avoids backing-buffer work for short map entries, and walks indexed unique
maps directly. It also reuses generator spellings and adjusts fragmented
compressed-frame growth. Those comparisons start at `0287879d`. The later
transport follow-up compares `71422f00` with three exact reviewed wire-file
overlays; the source manifests and final validation bind them separately.
The final compressed-only transport implementation is `6d1336e0`. Its identity
framing keeps the earlier growth policy after a broader reservation experiment
failed its CPU holdout.

Read the complete [timing table](timing.csv), [resource table](resources.csv)
and [remaining-loss gate](all-win-gate.json). Lower ratios favor pbrs.
The [results](results.json), [raw archive](raw.tar.gz), [manifest](manifest.json)
and [checker](check.py) retain the supporting records. See the
[benchmark guide](../../benchmarks.md) for the other workloads.

## Results

The first frozen implementation is `71422f00`, compared with `0287879d`.
Its original 94 codec/map/holdout rows remain separate from later prototypes.
Five randomized process pairs per row contain three samples per process;
CPU and allocation counters use separate binaries.

| Work | Before | After | Reference | Median paired after/reference |
|---|---:|---:|---:|---:|
| Lazy depth-64 parse/read/drop | 28,300 ns | 3,968 ns | 1,657 ns Prost | 2.339 |
| Owned depth-64 parse/read/drop | 1,507 ns | 1,502 ns | 1,583 ns Prost | 0.946 |
| Lazy mixed message, parse/read/drop | 864 ns | 802 ns | 895 ns Prost | 0.909 |
| Owned mixed message, parse/read/drop | 855 ns | 837 ns | 845 ns Prost | 0.991 |
| Warm unique map, 256 entries, iterate all | 1,943 ns | 94 ns | 253 ns standard HashMap | 0.368 |

The large unique-map paired CPU change is 0.049: 95.1% less CPU than the
previous revision. Lazy depth-64 full reading uses 86.4% less paired CPU but
still loses to Prost. At depth 64, a partial first-child
read falls from 1,633 to 852 ns, versus Prost's 1,558 ns (paired ratio 0.554).
The depth-eight partial read improves but still loses (1.281 times Prost).

The owned profile beats Prost's median in 15 of 16 parse/read rows; depth-64
parsing is near parity at 1.004. All 16 encoding holdout rows favor pbrs across both
profiles. Only three of 42 warm-map rows beat the HashMap control: full
iteration over unique maps with 16, 64 and 256 entries. Small maps and
large duplicate-heavy maps remain substantial losses. These are median
comparisons, not all-repeat or controlled-host claims.

The raw warm-map helper labels its standard HashMap control `prost`. It does
not run Prost decoding. Map construction and index warming are outside these
microbenchmarks; the full codec matrix includes parsing and dropping messages.
The complete records retain all rows, including ties, regressions and invalid
CPU slopes. Core and generator rows compare `0287879d` with `71422f00`;
the final transport rows compare `71422f00` with the three reviewed transport
files described below. These source scopes are explicit in every ledger row.

## Lazy decoding without repeated validation

The public parser still validates nested wire before accepting a message.
New generated code records that result on a deferred singular child. When a
getter constructs the child, its decoder reuses the existing validation result
and decodes the immediate fields normally. It does not recursively validate
the same descendants again. This removes the repeated traversal that made a
full read of a nested chain quadratic in the earlier implementation.

Children remain lazy. A first getter constructs one immediate child instead
of constructing the entire subtree. Unread children remain deferred, and
untouched cached wire can still be reused for encoding. Fresh merges take the
ordinary checked path, including after reads or mutation. Collection helpers
and old generated or handwritten `MergeBytes` implementations can also use the
existing checked fallback.

Regenerate application bindings to enable the new generated path. Old bindings
remain functional through the checked fallback. Three generated merge-helper
names are now reserved; a schema field with one of those names receives the
existing `_field` collision suffix.

An independent instrumented consumer checks a 64-node chain against Prost:
the initial parse enters 63 child validators, and the later chain getters
enter none. It also checks malformed wire, UTF-8, recursion limits, fresh
merges, required fields, oneofs, collection children, clone/mutation isolation,
concurrent getters and shared-buffer lifetime. Its counters are absent from
shipping benchmark binaries. It records 22 ordinary collection fallback
validations rather than claiming that every collection path skips validation.

The validation flag has a cost. In this proof, each affected lazy allocation
requests eight more bytes: parse traffic is 432 rather than 424 bytes, and the
first getter requests 128 rather than 120 bytes. These are requested allocator
bytes for the proof fixture, not RSS or a claim about every message layout.
The original Wire representation and generated all-zero default layout stay
intact. The rejected Wire-layout prototype and its failures remain documented
in the evidence; it is not the selected implementation.

The explicit `owned_message_fields(true)` profile remains available. It
constructs singular children while parsing and has different partial-read,
allocation and encoding costs. Compare both profiles for the application's
workload rather than treating one as a universal winner.

## Maps

Map-entry decoding now uses bounded slices and offsets into the parent input.
Numeric and inline-string-only entries do not create an intermediate backing
wire or window. Longer strings and message values retain the established
sharing policy and correct offsets; decoded views still outlive the caller's
input buffer safely. UTF-8, entry bounds and last-wins duplicate behavior are
checked independently.

A warmed last-wins index has one position per unique key. If its count equals
the original entry count, every key is unique. Iteration can then walk the
borrowed slice without searching the index for each entry. Its exact size
hint also reserves collection output once. This proof uses existing metadata;
it adds no map field, allocation or unsafe operation. Duplicate-containing and
unindexed maps retain their filtering path. Raw arena snapshots are unchanged.

Nine view tests and nine existing map units pass. A counted-key test verifies
zero comparisons after warming a unique index; an allocator probe verifies
zero iterator allocations. Mutation controls cover replacement, unique and
duplicate appends, removal with an earlier value resurfacing, mutable views
and clone/rebuild. Six separate map-decoding controls cover short and long
strings, multiple message offsets, malformed input and buffer lifetime.

## Generator costs

Per-worker field-name caches share `Rc<str>` spellings. Fixed scalar, map and
storage type spellings use borrowed `Cow<str>` values; formatted message paths
remain owned. This avoids repeatedly allocating the same temporary strings.
State remains worker-local, with no global intern table or added atomics.
Generated application code does not depend on `Rc`.

The isolated spelling-only comparison produces byte-identical files on all
six allocation corpora. It makes 13.99–22.06% fewer allocation requests and
requests 0.37–0.87% fewer bytes. Retained bytes are unchanged; one peak-live
case rises 5.74%. These instrumented captures overlapped compilation, so their
clocks cannot establish generator CPU performance or peak RSS. Final integrated
generation intentionally also includes the new lazy helpers described above;
the spelling-only equivalence check does not claim that all new bindings are
byte-identical to the older decoder output.

Twenty-five focused codegen units and strict core-library Clippy pass.
The canonical benchmark uses debug builds. Matched release captures use the
same release settings for both peers and remain a separate profile. CLI wall
time and sampled RSS do not measure downstream consumer cold/check/release
build costs; those remain an unmeasured axis for this source.

## Correcting generator timing

The original canonical timer used `Popen.wait(timeout=...)`. On POSIX this
polls process completion with exponential backoff, adding plateaus near
8, 16, 32, 64 and 114 milliseconds. Identical frozen binaries produced
substantially different reported wall medians when they crossed a polling
boundary. Those original generator wall results are diagnostic records and
must not support a speed claim.

The corrected timer uses a blocking completion wait, an independent deadline
watchdog and whole-process-group cleanup. It records completion before waiting
for RSS-sampler teardown. Seventy Python harness tests pass with one existing
skip; the two new regressions fail against the older helper. Commands, corpora,
build profiles and frozen binaries stay the same in the corrected comparison.
Codec process-CPU and RPC N/2N counters use different timing paths.

The corrected fresh-generation comparison is:

| Build profile | Messages | pbrs elapsed | Prost elapsed | Median paired pbrs/Prost |
|---|---:|---:|---:|---:|
| Canonical debug | Small corpus | 5.31 ms | 5.92 ms | 0.894 |
| Canonical debug | 100 | 15.66 ms | 31.03 ms | 0.505 |
| Canonical debug | 1,000 | 105.19 ms | 277.02 ms | 0.381 |
| Matched release | Small corpus | 4.44 ms | 3.98 ms | 1.054 |
| Matched release | 100 | 9.12 ms | 9.98 ms | 0.909 |
| Matched release | 1,000 | 63.26 ms | 79.19 ms | 0.798 |

All six canonical debug rows, including unchanged regeneration, have lower
pbrs medians than Prost. Four of six release rows do; small fresh and unchanged
cases lose slightly. Each displayed elapsed value is a separate median, so its
ratio need not equal the median paired ratio. These are local default-generator
comparisons, not proof of every generator or consumer-build workload.

Output volume is retained rather than normalized: the 1,000-message pbrs
output is 36.31 MB versus Prost's 0.413 MB. pbrs emits additional accessors,
reflection and format helpers. This is a real downstream cost; faster CLI
elapsed time does not establish smaller output or faster compilation.

A follow-up that replaced one spawned worker with the caller produced identical
files but mixed timing changes, so it was rejected. The small-map cache follow-up
also passed correctness and heap-footprint gates but regressed unique-map
iteration by 5.9–8.2 times. Neither prototype is selected. Their complete
94-row runtime and 12-row corrected generator comparisons remain in the archive.

## Transport buffers

Fragmented compressed frames validate the header and configured limit before
using its length. Once at least half the frame has arrived, the reader can
reserve the validated remainder before copying a large HTTP/2 DATA chunk.
A uniquely owned empty buffer is reclaimed first. Identity frames keep their
previous growth policy, and complete frames retain their sharing behavior.
Headers and tiny body fragments do not reserve a large claimed body.

Compressed output also keeps a reusable buffer tail between messages. The
next message can reclaim that storage once the previous immutable frame has
been consumed. Entirely compressed batches are eligible. Identity attempts
invalidate eligibility before calling the codec, including on failure. A
scope guard releases empty buffers on errors and unwinding while restoring
any complete earlier frames.

The original backing allocation, including its shared prefix, is charged to
the existing encode-retention budget: at most 4 MiB per retained backing and
16 MiB across retained encode buffers. These limits are unchanged. The policy
bounds idle retained buffers, not all active or process memory. Plaintext
serialization scratch and compressed output share that budget; decode scratch
has its existing independent budget.

### Final transport comparisons

These records compare `71422f00` with the exact three wire files in
`6d1336e0`. CPU has five randomized process pairs and allocation traffic has
two pairs, using separate binaries. MB below means 1,000,000 bytes. Allocation
counts, every pair and invalid slopes remain in the resource and timing tables.

| Work | Endpoint | Requested MB/RPC | Tonic MB/RPC | Paired bytes after/before | Paired CPU after/tonic |
|---|---|---:|---:|---:|---:|
| 4 × 1 MiB gzip noise, bidi | client | 22.288 | 22.774 | 0.722 | 1.055 |
| 4 × 1 MiB gzip noise, bidi | server | 21.229 | 21.720 | 0.712 | 0.964 |
| 4 × 1 MiB gzip zeros, bidi | client | 15.121 | 17.481 | 1.000 | 1.000 |
| 4 × 1 MiB gzip zeros, bidi | server | 14.062 | 16.427 | 1.000 | 0.667 |
| 4 × 1 MiB gzip noise, upload | client | 16.072 | 12.151 | 0.988 | 0.990 |
| 4 × 1 MiB gzip noise, upload | server | 12.859 | 14.377 | 0.860 | 1.000 |
| 4 × 1 MiB gzip noise, download | client | 17.111 | 17.516 | 0.891 | 1.000 |
| 4 × 1 MiB gzip noise, download | server | 21.075 | 18.557 | 0.902 | 1.000 |
| 1 MiB gzip noise, unary | client | 10.666 | 10.971 | 0.836 | 1.000 |
| 1 MiB gzip noise, unary | server | 10.670 | 10.970 | 0.836 | 1.074 |
| 8 MiB identity zeros, unary | client | 66.133 | 44.129 | 1.000 | 1.000 |
| 8 MiB identity zeros, unary | server | 66.138 | 44.129 | 1.000 | 1.000 |

The default 1 KiB unary and 64-message pipeline CPU holdouts remain separate:

| Work | Endpoint | Median paired native/tonic CPU | Pair range |
|---|---|---:|---:|
| unary_1k_prost | client | 0.349 | 0.290–0.383 |
| unary_1k_prost | server | 0.868 | 0.692–0.921 |
| pipeline_64x1k_prost | client | 0.723 | 0.650–1.600 |
| pipeline_64x1k_prost | server | 0.433 | 0.257–0.846 |

The pipeline client also has a higher paired before/after CPU median:
1.471 (range 0.944–2.167);
the server change is 1.158
(0.529–1.538). Fresh control slopes vary too.
These increases remain in the ledger; neither a lower after/tonic median nor
control variability establishes absence of a regression. The client loses one
after/tonic repeat. No general transport CPU improvement is claimed.

The longer identity confirmation keeps the same 8 MiB payload, 16 MiB caps,
peers and binaries, increasing counts from 10/20 to 500/1,000. It adds evidence
without replacing the original large-cell rows:

| Endpoint | Before CPU | Final CPU | Paired after/before | Pair range | Paired final/tonic |
|---|---:|---:|---:|---:|---:|
| client | 6.40 ms/RPC | 7.18 ms/RPC | 1.151 | 0.847–1.563 | 0.928 |
| server | 5.16 ms/RPC | 6.00 ms/RPC | 1.045 | 0.971–1.353 | 1.009 |

These CPU comparisons remain local shared-host measurements. Fresh controls
and pair ranges matter; allocator savings do not establish a general CPU win.
The original 10/20-RPC identity slopes are especially coarse at the 100 Hz
counter resolution. The complete ledger retains them all.

### Rejected framing and output attempts

The initial framer-only change in `71422f00` did not improve end-to-end
allocation bytes. A later reader-only experiment improved unary traffic but
regressed some streaming cases; reclaiming empty storage removed that issue.
An output-buffer prototype failed independent quota and unwind review and was
never timed or selected. Its reviewed replacement supplies the guarded output
reuse above.

A broader framing candidate, C, also anticipated identity lengths. It reduced
8 MiB unary requested bytes by 25.4%, but its longer 500/1,000-RPC check raised
client CPU medians from 7.12 to 11.00 ms and server medians from 5.74 to
9.20 ms. Median paired changes were 1.505 (range 0.764–2.455) and 1.560
(0.421–4.128). Tonic controls moved too; that does not dismiss the loss.
No CPU throttling or new memory-limit events occurred in the retained latter
host window. Identity anticipation was rejected and the final compressed-only
source was built and measured again. C's raw captures remain historical.

Closed uploads currently encode queued messages before sending them. Downloads
can begin the next compression while the connection still owns a small DATA
suffix of the previous backing. Both behaviors can prevent reuse and need
separate scheduling and ownership work. These explanations come from code
inspection; this campaign has no per-role cache-miss instrumentation.

Sixty-nine wire units and strict transport Clippy pass on the reviewed source.
An independent HTTP/2 peer checks nonce-distinct incompressible payloads in
both upload and native-server response directions. It covers mixed sizes,
oversized retention misses, twenty live streams, aggregate budget saturation,
cancellation, deadlines and clean completion. Every decoded message is checked
and all leases return to zero. Error, panic, identity and shared-prefix cases
also have unit controls. CI runs the permanent integration fixture explicitly.

## Validation and reconstruction

The first implementation passes all 26 final validation gates on unchanged
source inputs: 3,968 all-feature workspace tests, zero failures and nine existing
ignores; strict core/transport Clippy; owned and validated-lazy consumers; all
18 message-presence comparisons and 41 mixed-peer tests; Rust 1.85 native,
Tower, owned and validated-lazy checks; pinned descriptors; and documentation.

Both default lazy and isolated owned bindings pass three official conformance
runs each: 5,631 binary/JSON and 909 text cases, zero skips or failures, maximum
Edition 2023. Default regeneration changes seven of 13 bundled files; the other
six remain identical, and the final drift check passes. The standalone strict
Prost differential intentionally fails on the exact known wrong-wire-type and
missing-required-field policy differences; its known-differences mode passes.
This establishes the checked corpus, not complete behavior equivalence.

The final `6d1336e0` source passes all nine final checks, including
3,980 all-feature workspace tests (0 failures, 9 existing ignores),
strict transport/adapter/example Clippy, mixed-peer tests, Rust 1.85 native
and Tower builds, official default conformance and generated-file drift.
The Rust runtime/generator and non-transport proof inputs remain byte-identical
to the earlier complete validation. The isolated owned-conformance record
continues to name `71422f00`; it is not silently reassigned to the final source.
The initial final-validation attempt caught draft links to not-yet-packaged
assets; that failure and its disposition are retained. Publication checks
validate the completed links and every evidence checksum separately.

The archive retains frozen source manifests, exact overlays, build records,
raw captures, paired summaries, lockfiles, correctness prechecks, initial
failures and rejected attempts. Shipping timings use binaries without
instrumentation and run separately from allocation captures. The selected
94 core rows contain 1,880 shipping process captures and 5,640 samples, plus
376 separate allocator captures. Corrected selected generation uses 240 CLI
invocations across debug and release. The selected transport campaign has
684 captures and 1,942,560 successful RPCs, including
45,000 calls in the longer identity confirmation.

The final timing ledger has 124 rows: 69 wins, 48 losses and seven ties.
The 236 resource rows contain 43 wins, 55 losses and 138 ties; zero/zero
counts remain ties.
Nine benchmark axes remain missing, including downstream consumer-build costs.
All-benchmark victory is false. Median wins also do not imply every-repeat wins.

RPC captures use one connection, one RPC in flight and two Tokio workers per
endpoint on one shared x86_64 VM. They validate successful counts, lengths and
status, rather than reading every zero payload byte. There is no per-capture
FAIRNESS/socket attestation or independent host qualification. The 8 MiB cell
sets the same explicit 16 MiB message caps on both peers. CPU counters are
100 Hz N/2N differences; RSS, allocator traffic and CLI wall time are separate
metrics. Exact exported sources, ordered patches, overlays and reproduction
drivers are retained; run `python3 check.py` in this directory to verify them.

These records establish their measured workloads and source revisions. They
do not establish every possible workload, full tonic/prost parity or a world
performance ranking. Build elapsed times alone are not a generator-speed
comparison.
