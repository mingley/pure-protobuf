# Unary cancellation logging: exact 713 endpoint screen

This screen compares the proposed unary binary-log cancellation implementation
at `b683681518a277afc421dde3b3cd56e45c421cc0` with its parent runtime at
`713038c7f2d60da41824531a7b81920183c7510b`. It is an internal comparison,
not a tonic or Prost comparison.

The configured-logger construct-and-drop case was slower in every block,
including when the filter excluded the method. This is an unpolled-call
microbenchmark; it does not represent the cost of a completed RPC. The result
motivated a separate unpolled-drop experiment. No-logger and excluded endpoint
results overlap the same-binary controls. The enabled no-op-sink endpoint
ratios were above one in every block, while control ranges overlap those
differences. These results alone do not establish a precise causal effect.

## Method

[Run 42739333128](https://github.com/mingley/pure-protobuf/actions/runs/42739333128)
built both variants from pinned private runtime sources on a shared Intel Xeon
Platinum 8573C runner. The scratch commit was
`5de060169735eeae56a7894d068ed3a6c22eea92`; the workflow checked the source
hashes, retained binary hashes and deleted its scratch branch.

A single current-thread Tokio process ran a native client and server over TCP
loopback without TLS. There were 50 warmups per process. Each of ten balanced
ABBA or BAAB blocks measured two processes per variant and separate baseline
and candidate same-binary controls. Endpoint success ran 5,000 RPCs; endpoint
cancellation ran 1,000. Construction cases ran 5,000 calls: held calls stayed
unpolled and live until after timing; construct-and-drop calls were never
polled and included drop. CPU time is process CPU, and ratios compare the
sum of the two candidate measurements with the two baseline measurements.

Timed binaries did not enable explicit handler-count, allocation-count or
copy-count instrumentation. Success validates replies in the timed loop;
cancellation includes mpsc entry/drop coordination and cancellation checks.
Runtime atomics remain. The excluded sink was checked empty; the enabled
timing used a no-op sink and separate smoke runs checked emitted records.
Controls show the second measurement divided by the first. Ranges are observed
extremes, not confidence intervals.

## Process CPU

| Logger | Workload | Candidate / baseline | Slower blocks | Baseline control | Candidate control |
| --- | --- | --- | --- | --- | --- |
| No logger | Unary success | 1.0008 (0.9935–1.0085) | 6/10 | 0.9815–1.0131 | 0.9850–1.3125 |
| No logger | Unary cancel | 0.9915 (0.9747–1.0328) | 1/10 | 0.9936–1.0462 | 0.9369–1.0104 |
| No logger | Held construction | 1.0029 (0.9940–1.0235) | 6/10 | 0.9790–1.0150 | 0.9798–1.0204 |
| No logger | Construct and drop | 0.9886 (0.8760–1.0313) | 3/10 | 0.9719–1.0646 | 0.8242–1.0130 |
| Excluded | Unary success | 1.0022 (0.9895–1.0202) | 8/10 | 0.9619–1.0058 | 0.9925–1.0156 |
| Excluded | Unary cancel | 0.9944 (0.9921–1.0095) | 1/10 | 0.9784–1.0096 | 0.9849–1.0070 |
| Excluded | Held construction | 1.0057 (0.9814–1.0157) | 7/10 | 0.9883–1.0162 | 0.9862–1.0143 |
| Excluded | Construct and drop | 1.1853 (1.1565–1.2263) | 10/10 | 0.9392–1.0598 | 0.9287–1.0921 |
| Enabled | Unary success | 1.0098 (1.0035–1.0125) | 10/10 | 0.9825–1.0087 | 0.9790–1.0151 |
| Enabled | Unary cancel | 1.0078 (1.0003–1.0197) | 10/10 | 0.9453–1.0083 | 0.9900–1.0046 |
| Enabled | Held construction | 1.0065 (0.9973–1.0252) | 9/10 | 0.9761–1.0170 | 0.9926–1.0113 |
| Enabled | Construct and drop | 1.1955 (1.1706–1.2143) | 10/10 | 0.9296–1.0208 | 0.9634–1.0527 |

## Allocation requests and layout

Counting-allocator binaries were separate from timed binaries. Five warmed
`N`/`2N` pairs measured process-wide successful allocation requests for both
endpoints. Baseline and candidate matched in every pair:

| Logger | Workload | Allocations / RPC | Reallocations / RPC | Requested bytes / RPC |
| --- | --- | ---: | ---: | ---: |
| No logger or excluded | Unary success | 11.008 | 0 | 15,962.928 |
| No logger or excluded | Unary cancel | 26 | 0 | 18,562 |
| Enabled no-op sink | Unary success | 30.008 | 21 | 17,119.928 |
| Enabled no-op sink | Unary cancel | 38 | 15 | 19,223 |

Separate retained-unpolled construction pairs reserved vector capacity before
counting and sampled before calls were dropped. Across all three logger modes,
each additional call requested two allocations, zero reallocations and
5,720 bytes at baseline versus 5,728 bytes for the candidate. An instrumented
layout check found the unary future at 5,632 bytes for both and cancellation
`Inner` at 72 versus 80 bytes. These are requested bytes and type layouts,
not peak memory. A first cancellation on a logging-capable flag can allocate
boxed logging state; this construction check ends before drop.

## Evidence

[Raw measurements](unary-cancel-713-screen-20261110.json) retain every
process CPU and wall sample, block order, control, allocation pair, layout
and binary hash. The workflow's
[artifact](https://github.com/mingley/pure-protobuf/actions/runs/42739333128)
has ID `13728378449`, SHA-256
`19380dc9b482d6b4478a252e6cc08efc6ab2f499eefdf88e99f84cef4eb3b97f`,
and expires on 2026-11-24. [Independent verification](https://github.com/mingley/pure-protobuf/actions/runs/42742912726)
checked the source, artifact and documentation links. Earlier 9449-versus-1e
screens are historical comparisons of earlier source, not measurements of this
runtime.
