# Held packed-field borrowing experiment

Both candidates are held. They remove an allocation from fresh fixed-width
encoding, but repeated getters regress on the measured host. No packed runtime
change from this experiment ships on main. PK-14 remains open.

Read the [full results](RESULTS.md), [source pins](source-pins.json), or
[reproduction recipe](REPRODUCE.md). Complete [original](original-results.json)
and [refined](refined-results.json) reports retain every sample and control.

## Changes considered

The [original patch](patches/original.patch) adds a safe codec hook with a
default fallback to the existing encoded cache. Built-in little-endian
fixed-width codecs borrow their decoded storage through the existing concrete
scalar byte cast. A custom codec's public `MEMCPY_SAFE` flag never authorizes
a generic memory reinterpretation. Big-endian targets keep byte conversion.

The [refinement](patches/refinement.patch) reads the decoded memoization state
once and reuses it, preserving empty-field and wire precedence. The
[complete refined patch](patches/refined.patch) applies directly to baseline.
Neither candidate changes the checked packed-size bound or adds a generic
unsafe cast. Copy counters omit the eliminated encoded-cache copy; output
encoding can still copy bytes.

Fresh fixed32 fields request four allocations on baseline and three with
either candidate. The original warmed fixed32 getter is 7.7–8.2% slower at
three tested sizes, with all eight blocks slower. The refinement reduces that
cost to 3.9–4.4%, but all six blocks remain slower. Refined fixed64 and
wire-backed fixed32 getters are 6.3% and 10.8% slower. Cold and copy workloads
improve. These tradeoffs support holding both candidates.

## Measurement boundary

These are component screens on shared x86_64 Linux GitHub runners, with
affinity pinned to logical CPU 0. They use Rust 1.85.0 / LLVM 19.1.7 release
builds, one codegen unit and thin LTO. Timing binaries contain no allocation
counter. Separate instrumented binaries measure allocation requests through
three N-to-2N pairs; all pairs agree for every case.

Fixture preparation and two warmup operations run outside each sample. The
baseline calibrates a common iteration count toward 180 ms of thread CPU.
The original screen uses eight rotated, reversed six-process blocks for each
of 26 cases; its third variant independently rebuilds baseline. The follow-up
uses six blocks for 16 cases, comparing baseline, original and refined source.
Minimum observed process CPU is 106.402 ms and 110.478 ms respectively.
The raw `wire_len` describes the prepared fixture; the empty case separately
constructs an empty field and returns no bytes.

These measurements cover neither full generated messages nor the 1–5 MiB
qualification cells. They compare no upb, prost, tonic, RPC, TLS or server/client
workload. They establish no performance ranking or release qualification.
Canonical-varint reuse and the remaining PK-14 work are unfinished.

## Validation and retained evidence

The original passed 21 packed tests in default and runtime-only profiles on
stable and Rust 1.85, format checks, and native plus s390x Miri. Its
[combined run](https://github.com/mingley/pure-protobuf/actions/runs/44033461702)
also requested unsupported host alloc-only unit tests, which hit the crate's
explicit bare-metal profile guard. No runtime fix was needed.
[Separate checks](https://github.com/mingley/pure-protobuf/actions/runs/44034108876)
passed strict all-feature clippy and the supported Cortex alloc-only library.

The refined source passed the same 21 tests in both profiles on both
toolchains, formatting, strict clippy and both-endian Miri in
[44034936602](https://github.com/mingley/pure-protobuf/actions/runs/44034936602).
Its supported Cortex compilation passed in
[44035822951](https://github.com/mingley/pure-protobuf/actions/runs/44035822951).
Tests cover empty/clear/regrow, clones, mutation, canonical varints, float
payload bits and a padded custom codec.

The [original screen](https://github.com/mingley/pure-protobuf/actions/runs/44033791556)
and [follow-up](https://github.com/mingley/pure-protobuf/actions/runs/44035623544)
retain full artifacts, including six binaries, generated locks, environment
records and source. Their artifacts expire December 17, 2026; exact timestamps
and archive digests are in [source pins](source-pins.json).
The local JSON files preserve the complete stdout report as parsed JSON values
with formatting normalized. Recipes, source and patches remain in this folder.
All experiment scratch refs and worktrees were removed.
