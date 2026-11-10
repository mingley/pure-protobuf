# Streaming Call poll-state screen

This screen compares the unpolled-call cancellation prototype at
`bc3e22b09d0e5c2f47998c6a719effeeef3a6cca` with its
`b683681518a277afc421dde3b3cd56e45c421cc0` parent. It measures the
prototype's interaction with native client-streaming, server-streaming and
bidi RPCs. It is not a tonic comparison.

The observed candidate-to-baseline ratios overlap same-binary control ranges
in these workloads. The long enabled server-streaming case had a median
ratio of 1.0164, with seven of ten blocks above one; its control ranges also
span that result. The shared-runner variation limits conclusions about
small effects.

## Method

[Run 42743708411](https://github.com/mingley/pure-protobuf/actions/runs/42743708411)
built exact private runtime sources on a shared Intel Xeon Platinum 8573C
runner and deleted scratch commit
`272f81dcbfc0082f19d4747539a044b355433ea8`'s branch afterward. Each
process pinned itself to one CPU and used a current-thread Tokio native
client and interop server over loopback TCP without TLS.

For every logger mode and streaming shape, each process measured 500 streams
with four messages each or 20 streams with 128 messages each, after ten short
or two long warmup streams. Client-streaming polled the call in a spawned task
while sending 32-byte payloads. Server-streaming requested 32-byte responses.
Bidi sent one request and waited for its response repeatedly. Response sizes,
counts and stream termination were checked in the timed loop.

There were ten balanced ABBA or BAAB blocks and same-binary controls for each
case. CPU ratios compare the sum of the candidate pair with the baseline
pair. A control is its second process divided by its first; ranges are
observed extremes, not confidence intervals.

The timed probe had no explicit handler counters, allocation counters or copy
counters. Runtime atomics and the configured logging work remain. The enabled
mode used a no-op sink during timing and a collecting sink in separate
behavior checks; excluded sinks were checked empty.

The `Call` future for server-streaming and bidi completes after response
headers, so most long-stream message handling occurs in `Streaming`, outside
the changed `Call::poll`. These end-to-end results do not isolate the cost
per poll. Client-streaming keeps its call pending until its unary response.

## Process CPU

| Logger | Shape | Messages | Candidate / baseline | Slower blocks | Baseline control | Candidate control |
| --- | --- | ---: | --- | --- | --- | --- |
| No logger | Client streaming | 4 | 0.9940 (0.9070–1.0171) | 4/10 | 0.7698–1.1144 | 0.9820–1.4030 |
| No logger | Client streaming | 128 | 0.9953 (0.9478–1.0309) | 4/10 | 0.9690–1.0347 | 0.9626–1.0719 |
| No logger | Server streaming | 4 | 0.9994 (0.9811–1.0179) | 3/10 | 0.9610–1.0456 | 0.9723–1.0104 |
| No logger | Server streaming | 128 | 1.0067 (0.9917–1.0227) | 8/10 | 0.9722–1.0425 | 0.9262–1.0158 |
| No logger | Bidi | 4 | 1.0005 (0.9926–1.0108) | 5/10 | 0.9802–1.0439 | 0.9797–1.0104 |
| No logger | Bidi | 128 | 0.9976 (0.9894–1.0029) | 1/10 | 0.9806–1.0066 | 0.9832–1.0213 |
| Excluded | Client streaming | 4 | 0.9951 (0.9712–1.0065) | 2/10 | 0.9811–1.1199 | 0.9746–1.0180 |
| Excluded | Client streaming | 128 | 0.9790 (0.9561–1.0240) | 2/10 | 0.9936–1.0800 | 0.9462–1.0377 |
| Excluded | Server streaming | 4 | 1.0044 (0.9924–1.0157) | 6/10 | 0.9864–1.0654 | 0.9741–1.0097 |
| Excluded | Server streaming | 128 | 0.9912 (0.9593–1.0279) | 4/10 | 0.9672–1.0488 | 0.9597–1.0447 |
| Excluded | Bidi | 4 | 0.9982 (0.9867–1.0092) | 3/10 | 0.9869–1.0221 | 0.9855–1.0327 |
| Excluded | Bidi | 128 | 1.0013 (0.9290–1.0226) | 6/10 | 1.0010–1.0207 | 0.9903–1.0316 |
| Enabled | Client streaming | 4 | 0.9943 (0.9707–1.0153) | 1/10 | 0.9654–1.0198 | 0.9749–1.0359 |
| Enabled | Client streaming | 128 | 0.9864 (0.9674–1.0266) | 3/10 | 0.9586–1.0614 | 0.9393–1.0829 |
| Enabled | Server streaming | 4 | 1.0001 (0.9871–1.0288) | 5/10 | 0.9789–1.0207 | 0.9835–1.2350 |
| Enabled | Server streaming | 128 | 1.0164 (0.9829–1.0337) | 7/10 | 0.9716–1.0605 | 0.9691–1.0201 |
| Enabled | Bidi | 4 | 0.9983 (0.9803–1.0217) | 4/10 | 0.9500–1.0157 | 0.9800–1.0083 |
| Enabled | Bidi | 128 | 1.0024 (0.9865–1.0106) | 7/10 | 0.9891–1.0043 | 0.9854–1.0138 |

## Evidence

[Raw measurements](streaming-call-poll-screen-20261110.json) retain all
process CPU and wall times, order, controls, source and binary hashes.
The [run artifact](https://github.com/mingley/pure-protobuf/actions/runs/42743708411)
has ID `13730562070`, SHA-256
`bec4d066597eaffe786b760603ca14a941a3cc3d701ab11d69db13050216c5d4`,
and expires on 2026-11-24.
