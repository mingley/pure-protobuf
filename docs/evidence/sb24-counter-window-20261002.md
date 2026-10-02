# SB-24 cumulative CPU counter windows — 2026-10-02

Source `cbf72722fbebe4bff090c5c1e44658a8e46c4df9` corrects a second benchmark
headroom error after the [effective capacity correction](sb24-cpu-capacity-20261002.md).
An unweighted mean of per-sample CPU percentages can hide a long busy interval
among many short idle intervals. Headroom now uses cumulative endpoint CPU
counter deltas divided by that endpoint's actual initial/final snapshot interval.
The sample mean and peak remain diagnostic. Thread count does not affect the
capacity denominator, and the separate monitor/probe wall timer does not supply
the CPU counter interval.

The [original matrix guards](sb24-counter-window-20261002/matrix-before.log)
retain three failures: 9.6 CPU-seconds over ten seconds incorrectly passed at
1% CPU when one busy sample and 99 short idle samples were averaged; one CPU-second
over a two-second endpoint interval incorrectly used a ten-second monitor timer
and reported 10%; invalid intervals still reported a numeric average.
The corrected cases report 96% CPU, 4% spare capacity and saturation; 50% CPU;
and an unknown average with unverified headroom, respectively. The long busy
fixture's 1% sample mean remains visible without controlling the gate.

[Original stack guards](sb24-counter-window-20261002/stack-before.log) retain four
failures, including the same two-second versus ten-second denominator mismatch
and missing/reversed windows incorrectly certifying server headroom. The stack
gate now divides server CPU by its endpoint counter interval and preserves the
probe wall time as a diagnostic. Its 80% threshold and the generator's 95%
threshold are unchanged; no offered rate, lag threshold or failure gate changed.

Each endpoint resource record includes the exact initial/final monotonic timestamps,
duration, user/system counters and interval verification reason. Missing final
captures, nonfinite or nonpositive intervals, counter rollback and observed
nonmonotonic sampling transitions reject headroom. A last polling snapshot may
retain partial diagnostic counters but cannot substitute for a missing final
capture. Sequential reference processes preserve their separate counter windows;
summed counters are never paired with only the last process's timestamps.

[40 matrix tests](sb24-counter-window-20261002/matrix-after.log) and
[42 stack tests](sb24-counter-window-20261002/stack-after.log) pass, with two
credential-dependent stack skips. Both scenario self-tests, Python compilation
and diff checks pass. The two retained CLI invocation errors used the unsupported
`--selftest` spelling; corrected `--self-test` runs pass. These are recorded
separately from the red regression guards.

The [clean-source idle-process proof](sb24-counter-window-20261002/real-host-failclosed.json)
records valid client/server counter intervals of 0.210416431 and 0.212175289 seconds,
respectively, distinct from the monitor's 0.209802841-second timer. Both endpoints
are pinned to CPU 0 and have eight/two observed threads. Their cumulative CPU delta
is zero, but the hidden cgroup hierarchy still leaves effective capacity unknown;
the result remains **FAIL (UNVERIFIED)** with unknown spare capacity. This brief
functional fixture executes no RPC workload, build, throughput measurement or
headroom campaign.

[Full source/interpreter hashes and commands](sb24-counter-window-20261002/source-pins-and-commands.json)
and [artifact hashes](sb24-counter-window-20261002/artifact-sha256.json) pin the
reproduction. All earlier failed diagnostic rows and capacity evidence remain
unchanged. Setup/drain alignment with RPC measurement windows is still explicitly
unqualified. SB-24 remains open for required peers, effective runtime settings,
dedicated-host headroom, aligned windows and its remaining acceptance work.
