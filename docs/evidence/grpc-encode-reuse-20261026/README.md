# Bounded encoding and early streaming responses

Source `1d773f9787efd5dd389bb0f9896ba88c2eb1898e`, compared with
`e7595eb7640c60d5dfdaa708017ca9378e5e5332`. The changed source and tests are
recorded by file hash. These are Linux x86_64 shared-host
diagnostics using Rust 1.99.1; they do not establish an overall ranking, full
tonic/prost parity or production qualification.

Large compressed streams now reuse plaintext serialization storage through an
explicit append capability. Prost and field-wise generated pbrs messages opt in;
custom contiguous codecs keep their existing path. Idle large buffers charge
their full capacity to an independent 16 MiB process ledger, with a 4 MiB maximum
per buffer. Active encoding and the existing small scratch allowance remain
separate. Errors, panics and terminal call ownership release storage. Saturation
and oversize allocations are successful cache misses.

Compressed output starts with at most 4 KiB of lookahead. Once actual output
approaches half the known plaintext size, growth anticipates the full size plus
headroom. This avoids doubling again for an incompressible message's footer.
Smaller output grows normally; the target is a hint, not an output limit.
Wire bytes remain identical to independent gzip, deflate and optional Zstd
encoders, including noise and highly compressed 5 MiB bodies.

A separate repair preserves early server responses. Previously an empty closed
upload could encounter the peer's `RST_STREAM(NO_ERROR)` during its half-close,
then send a local CANCEL and lose a valid response or application rejection.
The client now stops uploading and validates the response. Missing/truncated
responses and other reset reasons still fail. A 1,000-round control fails on
the unchanged baseline at bidi round 33; the fixed stress test covers 3,000 calls,
including expected rejections, deadlines and an unconsumed sibling response.

The final allocation matrix completes **448 captures / 83,160 successful RPCs**,
with zero failed, timed-out, rejected, unstarted or unfinished benchmark calls.
Both sources have fresh tonic controls. Settings are plaintext loopback, two
Tokio workers per endpoint, one connection and one in-flight RPC, gzip level 6.
Each repeat uses N/2N process-allocation slopes: 500/1,000 calls for 1 KiB bodies,
10/20 for larger bodies, with two repeats and balanced source order. Noise bodies
are checked in full by both Prost peers. Pbrs endpoint cases use zero bodies.

**Requested MiB/RPC, not RSS; medians of two repeats.** The following streaming
cases use four messages, each carrying a 1 MiB body, with Prost at both endpoints.
Percentages are medians of paired changes.

| Native endpoint / shape | Payload | Before | Current | Change |
|---|---|---:|---:|---:|
| Client, bidi | Zero | 19.405 | 14.421 | −25.69% |
| Server, bidi | Zero | 18.395 | 13.411 | −27.10% |
| Client, bidi | Noise | 38.197 | 29.435 | −22.94% |
| Server, bidi | Noise | 37.187 | 28.425 | −23.56% |
| Client, upload | Zero | 11.766 | 7.277 | −38.15% |
| Server, server streaming | Zero | 14.773 | 10.285 | −30.38% |

Remaining losses stay visible. For four-message 1 MiB noise bidi, native requests
**35.53% more client bytes / 37.23% more server bytes** than fresh tonic/prost.
Zero-filled bidi is 13.50% / 14.39% lower. Native pbrs server bidi still requests
38.391 MiB/RPC; the pbrs/prost application comparison has different message
materialization costs and does not replace equal-Prost transport measurements.
Identity pipelined client bytes for 64 × 1 KiB increase **2.98%** versus baseline;
all repeated values, opposite endpoints, controls and smaller changes remain
in the complete slope records. Allocation changes are not CPU or speed claims.

The separate counters-off CPU campaign also completes: **180 captures / 272,160
successful RPCs**, with three repeats, no RPC failures and fresh tonic controls.
Native-server 1 KiB identity unary slopes are 30/56/38 µs per RPC, versus tonic's
30/24/30. The native paired median change versus baseline is −17.39%, but its
repeats span −25.00% to +27.27%. For 1 MiB noise unary, paired median CPU changes
are +6.90% client / +16.00% server; the server repeats span −7.14% to +22.22%.
The complete client/server and reference values remain in the record. These
shared-host observations, sampled at 10 ms CPU resolution, establish no general
CPU or latency improvement. The campaign completed before its terminal report
could be read after the execution connection recovered.

Validation on the frozen source:

| Check | Result |
|---|---:|
| Workspace, all features, including doctests | 3,855 passed, 0 failed, 9 existing ignores; 148 summaries |
| Strict core Clippy, all targets/features | Passed |
| Strict native library and both new integration targets | Passed |
| Warning-strict workspace rustdoc, all features | Passed |
| Formatting and source equivalence | Passed |
| Independent encode-cache integration | Reuse, 20 live streams, oversize miss, cancellation, deadline and cleanup passed |
| Early-response controls | Stress, early rejection, deadlines, malformed completion and non-OK resets passed |

The capsule retains preliminary failures and their successors: an ambiguous
associated constant fixed with explicit trait qualification, a consumer test
artifact-copy failure corrected by an absolute Cargo target directory, and
intermittent lifecycle failures minimized to the baseline early-response race.
Earlier successful runs are not relabeled as checks of later source. Official
conformance and sustained-resource records elsewhere retain their original pins.

[Results and complete comparisons](results.json) · [Integrity manifest](manifest.json)
· [Raw sources, builds, commands, logs and captures](raw.tar.gz)

Run `python3 check.py` to verify publication and archive hashes without extracting
files. The raw capsule includes a Git bundle covering the frozen implementation
and its predecessors; with the baseline already available:

```sh
tar -xzf raw.tar.gz source/final-source.bundle
git fetch source/final-source.bundle HEAD
git worktree add --detach ../encode-reuse 1d773f9787efd5dd389bb0f9896ba88c2eb1898e
```

Build records retain exact commands, lock hashes, Cargo artifacts and executable
hashes; ELF bytes and toolchain executables are omitted. The diagnostic drivers
are preserved by value. Cold/warm streams, all RPC shapes, 64-message pipeline
cases and all losses are included; separate hosts, overload, sustained resources,
broader platforms and full specification qualification remain open.
