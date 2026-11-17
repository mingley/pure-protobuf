# Owned scalar map values

The recorded candidate avoided a private parent Wire for map values
whose resolved type contains only singular scalar fields. Each borrowed parse
saves one allocation and a frame copy. Payload-bearing, repeated, nested, extern,
and MessageSet value types retain their existing policy.

Before publication, the initial broad predicate recorded here was narrowed to
enclosing schemas that cannot rely on parent backing for ordinary eager
descendants. See the [mixed-parent follow-up](../owned-parent-backing-20261117/README.md).
The positive timings below apply to the measured pure fixtures, whose generated
bodies remain unchanged by that narrowing. The raw data and original candidate
patch stay historical and unchanged.

On this hosted runner, borrowed owned decoding improved by 40.7%, 27.8%, and
27.6% for maps of 1, 32, and 512 entries. The 32-entry unknown-heavy case improved
by 19.1%. Their parse-and-touch improvements were 38.4%, 24.8%, 25.6%, and 18.9%.
All 34 rows, including controls, are in [results.md](results.md) and
[summary.json](summary.json).

These values measure elapsed time with Instant around warmed parse/drop loops,
on one pinned CPU. They are not CPU time, cycles, or instruction counts. Process
startup, corpus construction, one-time correctness checks, and the separate allocation
phase are outside the timed loop. Parse-and-touch additionally traverses every
decoded value and computes its checksum inside the timed loop; owned_decode
parses and drops the result. Both binaries retain
the same Counting allocator wrapper during timing, with counting disabled.

The schedule has four deterministic rounds. Cell order alternates forward and
reverse. Each cell has adjacent baseline/candidate processes, with side order
reversed for the latter two rounds, giving two baseline-first and two
candidate-first pairs. There is no separate A/A run or randomized schedule.
The default pbrs and Prost controls are unchanged-code controls in different
binaries; they cannot isolate host drift from code layout effects.

All shared scalar/unknown rows have unchanged allocation counts, with elapsed
medians within about 1.1%. All bytes-heavy rows also retain their allocation
counts. Their elapsed medians vary, including a 4.6% increase for borrowed
32-byte payload parse-and-touch. The 8192-byte rows range from -0.5% to +1.8%.
This screen supports the narrow allocation improvement; it does not establish a
universal no-regression result or dedicated-host performance qualification.

The Prost control uses prost 0.14.4 and the existing proof's BTreeMap selection.
Its results are retained as controls, not a ranking against Prost's default map
configuration. Default/lazy pbrs generated output was compared byte-for-byte
across both builds. The baseline fails the new scalar-map allocation assertion
with three events versus the candidate's two.

- [source-pins.json](source-pins.json) identifies both source commits, drivers,
  run/artifact URLs and digests, and production/fixture blobs.
- [harness.rs](harness.rs), [workflow.yml](workflow.yml), and the retained source
  patches describe the exact consumer, build, controls and loop.
- [raw-json.tar.gz](raw-json.tar.gz) preserves all 272 raw JSON outputs and their
  stderr files; [schedule.json](schedule.json) and [outcomes.json](outcomes.json)
  retain process order and commands.
- The proof logs retain borrowed/shared lifetimes, malformed boundaries,
  unknown-field round trips, required value merging/replacement, the baseline
  negative control, and Rust 1.85. The [correctness](correctness/) logs retain
  parser units and generated map-depth/required/unknown-group consumers.
- [check.py](check.py) checks hashes, all samples, deterministic allocation
  deltas, input identity, order balance and proof evidence. Run
  python3 check.py --negative-controls from this directory. It runs no new
  measurements.

All data was exported from existing artifacts; this record adds no samples.
