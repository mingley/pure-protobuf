# Synthetic Call polling screen

These component screens compare the `Call` poll-state prototype at
`bc3e22b09d0e5c2f47998c6a719effeeef3a6cca` with its
`b683681518a277afc421dde3b3cd56e45c421cc0` parent. The prototype is held
from integration. These are synthetic component measurements, not end-to-end
RPC or tonic comparisons.

First-Ready polling was slower in nine, nine and eight of ten blocks in
the three runs, although same-binary control ranges also include the reported
ratios. The first run's 250,000-call samples were short; the later runs sum
five and ten million first polls across batches. This direction merits
investigation, but the results do not establish a precise cause or RPC-level
effect.

## Method

Each build appended the same temporary public constructor to the private
request module and re-exported it for the diagnostic binary. The production
`Call::poll` implementation was otherwise source-pinned and unchanged. The
controlled boxed future performs four wrapping multiply, rotate and XOR
rounds, using `black_box`, then returns either Pending or Ready. There are no
per-poll measurement counters or allocation counters; the driver validates
each result inside timing.

Repeated Pending and synthetic Ready repoll each use one call for ten million
polls after 20,000 warmups. The Ready-repoll case intentionally uses a
repollable synthetic future; repolling a completed real RPC is not
recommended. First Ready polls preconstructed calls once each. The longer run
uses twenty batches of 250,000 calls; the independent repeat uses forty.
Both sum poll-only CPU and wall time. Construction and drop are outside the
intervals, while repeated clock reads and result checks remain. The workload includes memory access to the prepared
calls, so it does not isolate a single branch instruction.

Each case has ten balanced ABBA or BAAB blocks and same-binary controls.
Ratios use candidate pair CPU divided by baseline pair CPU; control ranges
compare the second process with the first. Sample CPU times below are medians
of the twenty baseline processes. Ranges are observed extremes, not confidence
intervals.

[The short run](https://github.com/mingley/pure-protobuf/actions/runs/42744949857)
used a shared AMD EPYC 9V45; [the longer run](https://github.com/mingley/pure-protobuf/actions/runs/42745580759)
used a shared AMD EPYC 9V74; and [the independent repeat](https://github.com/mingley/pure-protobuf/actions/runs/42746347933)
used a shared Intel Xeon 8573C. All pinned the source inputs and deleted their
scratch branches.

## Short first-Ready sample

| Workload | Polls / sample | Candidate / baseline | Slower blocks | Baseline control | Candidate control | Baseline CPU / sample |
| --- | ---: | --- | --- | --- | --- | ---: |
| Repeated Pending | 10,000,000 | 1.0026 (0.9585–1.0580) | 5/10 | 0.9511–1.0478 | 0.9741–1.0212 | 76.65 ms |
| First Ready | 250,000 | 1.0152 (0.9397–1.0558) | 9/10 | 0.9750–1.0473 | 0.9652–1.0319 | 2.79 ms |
| Synthetic Ready repoll | 10,000,000 | 1.0239 (0.9996–1.0483) | 9/10 | 0.9880–1.0629 | 0.9767–1.0667 | 80.03 ms |

## Longer first-Ready sample

| Workload | Polls / sample | Candidate / baseline | Slower blocks | Baseline control | Candidate control | Baseline CPU / sample |
| --- | ---: | --- | --- | --- | --- | ---: |
| Repeated Pending | 10,000,000 | 0.9998 (0.9863–1.0308) | 4/10 | 0.9698–1.0295 | 0.9737–1.0263 | 106.47 ms |
| First Ready | 5,000,000 | 1.0271 (0.9969–1.0339) | 9/10 | 0.9663–1.0420 | 0.8867–1.0356 | 58.60 ms |
| Synthetic Ready repoll | 10,000,000 | 0.9954 (0.9834–1.0165) | 1/10 | 0.9665–1.0273 | 0.9715–1.0299 | 113.78 ms |

## Independent longer repeat

| Workload | Polls / sample | Candidate / baseline | Slower blocks | Baseline control | Candidate control | Baseline CPU / sample |
| --- | ---: | --- | --- | --- | --- | ---: |
| Repeated Pending | 20,000,000 | 0.9954 (0.9771–1.0155) | 2/10 | 0.9661–1.1097 | 0.9658–1.0325 | 186.01 ms |
| First Ready | 10,000,000 | 1.0163 (0.9944–1.0599) | 8/10 | 0.9514–1.0745 | 0.9536–1.0476 | 94.39 ms |
| Synthetic Ready repoll | 20,000,000 | 0.9937 (0.9332–1.0440) | 5/10 | 0.8142–1.0462 | 0.9455–1.0559 | 202.16 ms |

## Evidence

[Raw measurements](call-poll-component-screen-20261110.json) retain each
process CPU and wall sample, block order, controls, input and binary hashes.
The short-run artifact ID is `13730448587`, SHA-256
`959364ec9abe97d38f3dbb10d9a2876b2e1d56c668aa4ee72affeac3bcd5cfde`;
the longer-run artifact ID is `13731132897`, SHA-256
`9a91afa1e281e887542fd056558852ca7da2f657432ecb3a31ec8f02738ef465`;
and the repeat artifact ID is `13732160732`, SHA-256
`6bbafa27dad0e2ea5d046944f132172b27180ce1a0f93acf48eccfac26fca890`.
All expire on 2026-11-24.
