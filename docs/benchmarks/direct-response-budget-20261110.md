[Benchmarks](../benchmarks.md)

# Small-response budget: direct runtime screens

The default client can reserve one queued, nonempty, non-final DATA frame
shorter than 256 visible bytes per stream, up to 1,024 frames per connection.
This can keep a burst of briefly unpolled responses from exhausting the base
frame-overhead budget. An explicit `ChannelConfig::data_frame_budget`
disables the reserve. The configured budget remains a shared cap for the
overhead it accounts for; reading a small frame can repay some previously
charged discarded work. A frame can still be rejected before a read makes
progress. See [resource budgets](../resource-budgets.md) for the full policy.

The reserve adds at most 1,024 × 255 = 261,120 bytes of visible DATA payload,
plus event and stream metadata and potentially larger shared backing
allocations. This is a logical-payload bound, not a peak-heap or total-memory
bound.

## What was measured

The change was shipped in [0685d7f](https://github.com/mingley/pure-protobuf/commit/0685d7f6cbc4e0ac9cdfb020bd085a8b7eb5b2be).
The screens compared exact baseline `41d94325b8aa48b10b31fb6709c1a41dbc44a72a`
with candidate `f1925f2a5a2e3fc759d02f2e0a1f754250c2f9c2`. The candidate’s
transport code matches the shipped change. Comparing both baseline and
candidate to their shipped counterparts finds only documentation and the
separate interop test helper and binary differences.

In each table, default leaves the measured component’s `data_frame_budget`
unset; explicit sets it to 512 KiB. The fixed peer keeps the same source and
settings across variants.

All tables show candidate CPU divided by baseline CPU: the median followed by
the observed range of paired blocks. Lower values are faster. These are
shared-runner samples over one loopback connection with client and peer on SMT
siblings; the ranges
are not confidence intervals. Each screen alternates the order and records
same-binary controls. Those control ranges overlap the observed effects. The
samples do not establish a general speedup or rule out a small cost. They do
not compare against tonic or Prost.

## Client CPU

The [client screen](https://github.com/mingley/pure-protobuf/actions/runs/42761887596)
used an AMD EPYC 9V74, a fixed baseline upstream-h2 peer and ten blocks of
10,000 calls per cell. Paired calls start two attempts and cancel the loser;
the peer checks the cancellation. It does not deliberately send late DATA.

| Calls | Client budget | Concurrency 1 | Concurrency 64 |
|---|---|---:|---:|
| Ordinary unary | default | 0.9889 (0.9435–1.0257) | 0.9747 (0.8984–1.0076) |
| Ordinary unary | explicit | 1.0045 (0.9606–1.0287) | 0.9796 (0.9177–1.0452) |
| Paired cancellation | default | 0.9900 (0.9598–1.0189) | 0.9985 (0.9808–1.0334) |
| Paired cancellation | explicit | 0.9985 (0.9847–1.0355) | 1.0054 (0.9451–1.0468) |

Separate instrumented 512/1,024-call deltas had median estimates of about
20.4–20.6 allocation calls and 8,222–8,267 requested bytes per RPC across
settings and variants. These are allocation requests, not peak live memory.

## Native server CPU

The [native-server screen](https://github.com/mingley/pure-protobuf/actions/runs/42761886834)
used an AMD EPYC 9V74, a fixed baseline native client and five blocks of 6,000
calls per cell. A separately instrumented build observed the zero-reserve
small-DATA path during client streaming for both server settings; it did not
observe that path in unary calls.

| Calls | Server budget | Concurrency 16 | Concurrency 64 |
|---|---|---:|---:|
| Unary | default | 0.9821 (0.9457–1.0922) | 1.0163 (0.9534–1.0326) |
| Unary | explicit | 1.0209 (0.9837–1.0461) | 0.9779 (0.9497–1.0475) |
| Client streaming | default | 0.9914 (0.9496–1.0900) | 0.9836 (0.9808–1.0826) |
| Client streaming | explicit | 1.0135 (0.9797–1.0314) | 1.0118 (0.9873–1.0412) |

Separate allocation snapshots used 1,000/2,000-call differences. Unary medians
were about 14.7–14.9 allocation calls and 12.5 kB requested per RPC;
client-streaming medians were about 19.8–19.9 calls and 13.1 kB. These noisy
process-wide estimates are not peak live memory.

## Native server with paired cancellation

A [separate screen](https://github.com/mingley/pure-protobuf/actions/runs/42762603034)
used an Intel Xeon Platinum 8573C and a fixed baseline client. The server holds
the first handler, responds to the second, and waits for the first handler’s
cancellation signal. The controls observed two streams, one response and one
cancellation per call; disabling the cancellation watcher failed the witness.
The server advertises 1,024 concurrent streams. Its timed work includes the
fixture’s map, per-loser watcher tasks and counters. The screen used six blocks
of 6,000 calls per cell.

| Server budget | Concurrency 1 | Concurrency 64 |
|---|---:|---:|
| default | 1.0022 (0.9809–1.0584) | 1.0154 (0.9544–1.0341) |
| explicit | 1.0048 (0.9923–1.0213) | 0.9674 (0.9258–0.9938) |

## Reproduce and check the evidence

[Raw data](direct-response-budget-20261110.json) includes every retained
candidate/baseline pair, allocation row and same-binary control ratio.
The two individual CPU readings behind each same-binary control ratio were not
retained, so the verifier can check the reported ratios and their summary
ranges but cannot recompute those ratios.

The [independent verification run](https://github.com/mingley/pure-protobuf/actions/runs/42764133928)
checked the three original artifact hashes, source ancestry and equivalence,
all 144 paired CPU ratios, 60 allocation rows, control ranges and the
instrumented witnesses. Its [90-day evidence bundle](https://github.com/mingley/pure-protobuf/actions/runs/42764133928/artifacts/13739166558)
(SHA-256 `289de2f37fccd42fb903fbfbf0cbfaa659be5505d2e5b6b44ecc7abfda0a842d`)
also retains the fixtures, workflows and logs. To rerun the
[verifier](direct-response-budget-20261110-verify.py) while that artifact is
available:

```sh
gh run download 42764133928 --name verified-direct-budget-evidence-20261110 --dir evidence
python3 evidence/verify_full_budget.py
```

In the exact-source release-layout check on x86-64, `Stream` stayed at
328 bytes, `DataEvent` at 40 bytes and `ChannelConfig` at 368 bytes.
Connection `Inner` grew from 712 to 720 bytes, including `Counts` from
176 to 184 bytes; the HTTP/2 client builder grew from 152 to 160 bytes.
These are layout measurements, not peak-memory estimates.
