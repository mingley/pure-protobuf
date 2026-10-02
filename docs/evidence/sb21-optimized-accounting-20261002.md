# SB-21 optimized aggregate-accounting replay — 2026-10-02

Seven short fixed-5,000 aggregate-QPS rows passed independent accounting with
zero rejected, failed or timed-out calls. Native release and pinned optimized
Go remove the earlier debug-native CPU saturation artifact. This is a loopback
accounting diagnostic; SB-21 remains open for a valid two-harness smoke with
verified generator/reference headroom and full benchmark qualification.

The unchanged native release binary was built at source
`1e6b1119182e1e4e90f2d93656765b3bf4929b81` with SHA-256
`7fdbe0be0ddf38280706cb4eda5f43915d2381c4338aaf844092357d2a3b6ff8`.
The clean measurement harness was
`30da0badd14b027d1004b0889c98eb5479978707`. All Go/overlay/driver hashes
match the [previous diagnostic](sb21-arrival-accounting-20261002.md).
The `--skip-build` runner itself correctly leaves native source unverified;
the independent release-build log and binary digest tie it to the source pin.
The SB-24 sampler edits were isolated in a separate worktree throughout.

[Raw results and commands](sb21-optimized-accounting-20261002/optimized-diagnostic.sh),
[machine summary](sb21-optimized-accounting-20261002/diagnostic-summary.json),
[artifact hashes](sb21-optimized-accounting-20261002/artifact-sha256.json) and
[lease timings](sb21-optimized-accounting-20261002/lease.json) retain every row.
Two randomized repeats ran native/native and both mixed directions, then one
Go/Go pair, at the frozen 5,000 aggregate offered rate and 64-slot limit, with
explicit 1-second warmup and 2-second measurement overrides. Both commands
returned zero. The coordinated quiet lease lasted **24.7597 seconds** and was
released immediately after the measurement shell completed.

| Client → server | Repeat | Offered | Completed | Unfinished | Completion QPS | Client CPU % | Server CPU % |
|---|---:|---:|---:|---:|---:|---:|---:|
| native → native | 1 | 10188 | 10177 | 11 | 5071.8 | 16.5 | 13.6 |
| native → Go | 1 | 10168 | 10168 | 0 | 5071.0 | 16.2 | 30.9 |
| Go → native | 1 | 10150 | 10151 | 0 | 5071.2 | 51.9 | 17.7 |
| native → native | 2 | 10180 | 10180 | 0 | 5075.1 | 17.1 | 14.0 |
| native → Go | 2 | 10168 | 10169 | 0 | 5070.1 | 16.5 | 30.6 |
| Go → native | 2 | 10143 | 10147 | 0 | 5071.6 | 51.6 | 18.2 |
| Go → Go | 1 | 10148 | 10143 | 6 | 5068.5 | 54.7 | 42.3 |

Carry-in and unfinished counters reconcile completions against dispatches over
each exact driver mark window. The fixed realized Poisson sequence has a short
window offered rate near 5,070; the nominal rate remains 5,000. Counts are not
corrected to an expected total. Both service and scheduled-send histograms
reconcile with completed calls, and effective scenario metadata is verified.
Every proof retains `claim_eligible=false`.

The frozen harness still has the old thread-count denominator; its headroom
outputs cannot qualify capacity. Official driver CPU percentages above preserve
their endpoint mark windows and are raw core-seconds per wall-second, without
an independently verified available-CPU denominator. The retained
[post-run CPU observation](sb21-optimized-accounting-20261002/post-run-cpu-observations.json)
shows affinity CPUs 0–4, cgroup membership `0::/`, mount root `/..` and visible
`cpu.max` of `400000 100000`. That mapping is ambiguous: the visible quota file
is a diagnostic observation, not a certified per-PID capacity. No headroom or
performance assertion follows from these lower CPU values.

All prior failed original-Go, debug-native, frozen-stack and strict-Clippy runs
remain in their historical evidence. Thresholds, shipping code, wire policy and
dependencies are unchanged. Dedicated hosts, aligned windows, broader peer
coverage, valid stack smoke, headroom and contract confidence/sample requirements
remain outstanding.
