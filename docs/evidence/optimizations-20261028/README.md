# Transport correctness and optimization review, October 28

This round repairs transport counter and frame-length overflow. It rejects
eight optimization candidates after broader measurements exposed regressions.
The goal of beating tonic/prost on every benchmark remains unmet: the selected
ledger has **160 timing rows with 80 nonwins**, **320 resource rows with 271
nonwins**, and **nine missing axes**. Full feature parity and a world ranking
are not established.

[Timing rows](timing.csv) · [Resource rows](resources.csv) ·
[Remaining losses and gaps](all-win-gate.json) · [Source and validation record](results.json)
· [Raw archive](raw.tar.gz) · [Benchmark guide](../../benchmarks.md)

## Source and selected changes

The measured source is `ce0a06e50ab2262cb3a38d39660858b30561fc88`, compared
with the frozen `6d1336e0` runtime. The latter's relevant inputs match the
published starting point `acc8aef9`. The validated source is
`cf97c83b3aec88ce1ca7ba7c6b23baa877bb9cf9`: it changes one map-test closure
to fix a lint and leaves measured production code unchanged. Exact hashes
and the test-only difference are in [results.json](results.json).

Checked atomic updates prevent unlimited byte and permit-token counters from
briefly publishing wrapped values. Outbound framing checks header arithmetic
and identity wire/allocation bounds before reservation or codec execution.
Encoding caps keep error precedence. Compressed plaintext retains its separate
length rules; earlier server replies still precede a later error trailer.
No retention cap is raised. Map, lazy-message and generator production designs
remain at their earlier implementations.

## Selected measurements

Values are median paired ratios; lower favors pbrs. All samples and pair ranges
remain in the archive. All 684 RPC captures use Prost on both client and server
for the native and tonic stacks, with fresh tonic controls and separate
CPU/allocation builds.

| Work | Selected result |
|---|---|
| Twelve large shipping RPC endpoint CPU rows | Five wins, five losses, two ties against tonic |
| Four default unary/pipeline CPU rows | Four wins against tonic |
| Longer 8 MiB identity client/server CPU | 1.494/1.654 times tonic; pair ranges 0.699–2.455/0.927–1.878 |
| Forty-two warm-map CPU rows | Five wins, 37 losses against standard HashMap |
| Lazy depth-8/64 parse, read and drop | 2.925/2.382 times Prost CPU |
| Owned mixed parse, read and drop | 1.095 times Prost CPU; pair range 1.066–1.142 |
| Release fresh generation, small/100/1,000 messages | 1.196/0.968/0.809 times Prost elapsed time |

The 1,000-message output is 36,310,287 bytes versus Prost's 412,622 bytes.
Generation time excludes consumer compilation. Map-only controls are standard
HashMap, not Prost decoding; warm maps exclude construction and indexing. The
added ordinary-loop holdout also measures construction/read/drop from an entry
list, which is not protobuf parsing.

The selected timing ledger contains 80 wins, 78 losses and two ties. Resources
contain 49 wins, 84 losses and 187 ties; zero/zero allocation rows count as ties.
Counts describe medians, not conclusive wins across every pair. CPU, requested
allocation bytes, calls and RSS are different metrics.

Core source is unchanged. Separately compiled executables can differ in code
layout, and the shared host adds noise. Changes from earlier core measurements
do not establish source-level speed changes. The transport repairs are
correctness fixes, not a demonstrated general speed improvement.

## Rejected designs

These candidates are retained as experiments, not shipped optimizations.
Ratios below compare candidate CPU with its prior implementation unless elapsed
time is stated.

| Candidate | Reason for rejection |
|---|---|
| Private lazy validation enum | Depth-8/64 full reads regress to 1.066/1.075 despite smaller measured boxes. |
| Initial map prefix/fold changes | Distinct ordinary loops regress to 5.473–7.547. |
| Narrow map changes | Distinct ordinary loops regress to 5.802–8.226. |
| Forced-inline map changes | Distinct collection regresses to 1.306–1.449; ordinary loops to 2.568–3.364. |
| First-survivor-only inline map changes | Distinct collection regresses to 1.377–1.448; ordinary loops to 2.479–3.371. |
| Generated output move-and-shrink | Small fresh release elapsed ratio is 1.171, with pairs spanning 0.825–1.266; no broad speed win. |
| Broad borrowed-wire helpers | Owned scalar parse/read regress to 1.269/1.174 despite faster lazy nested reads. |
| Narrow borrowed-wire helpers | Owned mixed parse/read regress to 1.102/1.101 and small fresh release generation to 1.102. |

Focused correctness checks passed. Removing clones, improving one map
distribution or forcing inlining did not prove a faster full lifecycle.
The rejected lazy enum saves eight requested bytes in this x86_64 fixture;
the layout depends on compiler and target. Move-and-shrink lowers some measured
memory peaks, but its allocator counts the full new size on realloc and cannot
observe transient old/new storage overlap inside the allocator. Direct-command
maximum RSS is not simultaneous process-tree peak memory.

Closed-upload deferral/interleaving was also rejected on source-level error
ordering grounds: early peer success could hide a later local codec error,
or DATA could become observable before that error. The discarded deferred
prototype has no exact retained source or timed claim. A multi-frame compression
slab remains unmeasured and needs a bound on concurrent active speculation;
the existing idle-cache ledger does not bound all active memory. The archived
`work/transport-closed-upload-analysis.md` explains the alternatives.

## Correctness checks

At `ce0a06e5`, the all-feature workspace run passes 3,992 tests with zero
failures and nine existing ignores. Fifteen tracker units, 21 framing units
and three framing integrations cover overflow, permits, prior replies, trailers
and recovery. The affected map tests pass again after the `cf97c83b` lint fix.
Strict lints, Rust 1.85 profiles, mixed peers, message presence, owned and
validated-lazy consumers, and regeneration checks pass.

At `cf97c83b`, two required and one recommended official conformance runs each
pass 5,631 binary/JSON and 909 text cases, with zero failures or skips and
maximum Edition 2023. Recorded non-Markdown inputs match. The report marks the
tree dirty; the recorded changes were `TODO.md` and two documentation files.
Strict Prost comparisons still fail on known
behavior differences in both profiles; known-differences mode passes. This
does not prove full Prost equivalence. Actual 32-bit execution or allocations
above 4 GiB were not tested; synthetic lengths exercise the boundary arithmetic.

## Reproduce and inspect

From the repository root, verify the retained archive without extraction,
Rust builds or network access:

```sh
python3 docs/evidence/optimizations-20261028/check.py
```

The checker verifies [manifest.json](manifest.json), result/companion hashes,
every archive member and the absence of ELF executables. It checks integrity;
it does not rerun benchmarks. After extraction, `source/manifest.json`,
`source/overlays/` and `source/reconstruct.py` preserve exact candidate inputs.
Build manifests, lockfiles, binary hashes, commands, raw samples and summaries
are retained under `work/all-benchmarks/measurement/` and
`work/benchmark-followup/`. Preparation/measurement entry points include
`freeze.py`, `measure.sh` and `measure-selected.sh` in the latter directory.
Validation logs are under `work/ce0a-validation/` and `work/cf97-validation/`.
Rejected stale-artifact preflights and interrupted checks are retained separately
from accepted captures.

The nine missing axes are consumer cold, incremental and release builds;
service-stub generation/consumer costs; a Prost generator-allocation control;
the complete serialization/construction survey; realistic codegen corpora;
RPC TLS/varied load; and dedicated-host performance qualification. These
plaintext, fixed-load, shared-host runs cannot close those gaps. Earlier
source-pinned losses remain in the [October 27 record](../decode-and-map-speed-20261027/README.md).
