# Unpolled-call cancellation screen

These screens evaluate the unpolled-drop prototype at
`bc3e22b09d0e5c2f47998c6a719effeeef3a6cca`. One compares it with its
binary-log cancellation parent `b683681518a277afc421dde3b3cd56e45c421cc0`;
the other compares it directly with main at
`713038c7f2d60da41824531a7b81920183c7510b`. They are internal comparisons,
not tonic or Prost comparisons.

The two runs used different shared hosts and must not be multiplied together.
In both, endpoint ratios overlap same-binary controls. The direct comparison's
enabled-logger construct-and-drop median was 1.0297 with eight of ten blocks
above one, but the control ranges include substantial variation. The data do
not establish a precise residual cost.

An earlier [no-capture early-filter experiment](unary-filter-no-capture-screen-20261110.md)
measured a slower excluded-method held-call construction in every block
without a decisive endpoint win. It used earlier source and is not the
prototype measured here.

## Method

Each process ran a native client and server on a current-thread Tokio runtime
over loopback TCP, without TLS. There were 50 warmups per process. Each case
used ten balanced ABBA or BAAB blocks with baseline and candidate same-binary
controls. Ratios compare the sums of the two candidate and two baseline
process CPU measurements. Controls compare the second process with the first;
ranges are observed extremes, not confidence intervals.

Success used 5,000 unary RPCs, cancellation used 1,000, and construction used
5,000 calls. Held calls stayed unpolled and live until after timing;
construct-and-drop calls were never polled. Success reply checks and
cancellation mpsc entry/drop coordination are timed. Explicit handler-count,
allocation-count and copy-count instrumentation was absent from timed
binaries; runtime atomics remain. Enabled logging used a no-op sink in timing,
with separate collecting-sink smoke checks. Excluded sinks were checked empty.

[The b683 comparison](https://github.com/mingley/pure-protobuf/actions/runs/42743140647)
ran on a shared AMD EPYC 7763. [The direct 713 comparison](https://github.com/mingley/pure-protobuf/actions/runs/42744393802)
ran on a shared AMD EPYC 9V74. Both pinned the private runtime sources and
deleted their scratch branches.

## Candidate versus b683

| Logger | Workload | Candidate / baseline | Slower blocks | Baseline control | Candidate control |
| --- | --- | --- | --- | --- | --- |
| No logger | Unary success | 1.0003 (0.9738–1.0302) | 5/10 | 0.9622–1.0448 | 0.9743–1.0306 |
| No logger | Unary cancel | 1.0101 (0.9608–1.0291) | 9/10 | 0.9775–1.0371 | 0.9571–1.0309 |
| No logger | Held construction | 0.9975 (0.9635–1.0044) | 5/10 | 0.9515–1.0080 | 0.9835–1.0215 |
| No logger | Construct and drop | 0.9894 (0.9064–1.1784) | 5/10 | 0.8562–1.1688 | 0.0790–1.2075 |
| Excluded | Unary success | 0.9913 (0.8845–1.0096) | 4/10 | 0.8825–1.0498 | 0.9714–1.0197 |
| Excluded | Unary cancel | 1.0068 (0.9812–1.0293) | 6/10 | 0.9704–1.0182 | 0.9840–1.0208 |
| Excluded | Held construction | 0.9966 (0.9739–1.0103) | 3/10 | 0.9770–1.0223 | 0.9672–1.0223 |
| Excluded | Construct and drop | 0.9169 (0.8450–1.2230) | 3/10 | 0.8790–1.2475 | 0.8395–1.4779 |
| Enabled | Unary success | 0.9913 (0.9766–1.0233) | 4/10 | 0.9819–1.0191 | 0.9819–1.0204 |
| Enabled | Unary cancel | 0.9938 (0.9490–1.0122) | 3/10 | 0.9887–1.0430 | 0.9758–1.0839 |
| Enabled | Held construction | 0.9978 (0.9158–1.0126) | 4/10 | 0.9883–1.0307 | 0.9757–1.0139 |
| Enabled | Construct and drop | 0.9787 (0.8395–1.5113) | 4/10 | 0.8627–1.2832 | 0.8248–1.3347 |

## Candidate versus 713

| Logger | Workload | Candidate / baseline | Slower blocks | Baseline control | Candidate control |
| --- | --- | --- | --- | --- | --- |
| No logger | Unary success | 0.9993 (0.9871–1.0221) | 5/10 | 0.9871–1.0293 | 0.9796–1.0330 |
| No logger | Unary cancel | 0.9977 (0.9781–1.0113) | 4/10 | 0.9746–1.0341 | 0.9858–1.0220 |
| No logger | Held construction | 1.0028 (0.9754–1.0370) | 6/10 | 0.9529–1.0142 | 0.9516–1.0220 |
| No logger | Construct and drop | 1.0009 (0.8734–1.2414) | 5/10 | 0.9530–1.0929 | 0.7269–1.0103 |
| Excluded | Unary success | 1.0008 (0.9931–1.0204) | 6/10 | 0.9878–1.0314 | 0.7957–1.0208 |
| Excluded | Unary cancel | 0.9948 (0.9919–1.1051) | 3/10 | 0.9708–1.0119 | 0.9863–1.7694 |
| Excluded | Held construction | 1.0038 (0.9742–1.0206) | 6/10 | 0.9794–1.0207 | 0.9760–1.0422 |
| Excluded | Construct and drop | 1.0059 (0.9251–1.0605) | 7/10 | 0.7501–1.0437 | 0.9536–1.1299 |
| Enabled | Unary success | 1.0006 (0.9856–1.0109) | 6/10 | 0.9608–1.0301 | 0.9786–1.0267 |
| Enabled | Unary cancel | 1.0014 (0.9775–1.0452) | 5/10 | 0.9824–1.0459 | 0.9673–1.0323 |
| Enabled | Held construction | 1.0069 (0.9834–1.0243) | 8/10 | 0.9945–1.0164 | 0.9669–1.0194 |
| Enabled | Construct and drop | 1.0297 (0.6344–1.0794) | 8/10 | 0.9609–1.0214 | 0.9121–1.1109 |

## Allocation requests and layouts

Separate counting-allocator binaries measured warmed `N`/`2N` differences
over five pairs. In both comparisons, no-logger and excluded success
requested 11.008 allocations, zero reallocations and 15,962.928 bytes per
RPC for both variants; cancellation requested 26 allocations, zero
reallocations and 18,562 bytes. Enabled allocation counts matched at
30.008 allocations and 21 reallocations for success, and 38 allocations
and 15 reallocations for cancellation. Enabled requested-byte differences
varied slightly between pairs; the raw data retain every value.

Separate retained-unpolled checks reserved vector capacity before counting
and sampled before drops. Every additional call requested two allocations and
no reallocations. Against b683, both requested 5,728 bytes. Against 713, the
baseline requested 5,720 and the candidate 5,728. Instrumented layouts were
`Call` 32 and the unary future 5,632 bytes for all three variants; cancellation
`Inner` was 72 bytes on 713 and 80 on b683 and the candidate. These are
requested bytes and type sizes, not peak memory. A first logging-capable
cancellation may allocate boxed state after this construction snapshot.

## Evidence

[Raw measurements](unary-unpolled-drop-screen-20261110.json) retain all CPU
and wall samples, control orders, allocation pairs, layouts and binary hashes.
The b683 run's artifact ID is `13730807074`, SHA-256
`1275b243a62b750b905e8a3146d81a02b1a4277a81fbc694b6f8ebf292652391`;
the 713 run's artifact ID is `13731491582`, SHA-256
`1eef61e20dc3307b741a4d7de32a4b4f65da559b4db6d8e0cdd2b4ec219a31ae`.
Both expire on 2026-11-24.
