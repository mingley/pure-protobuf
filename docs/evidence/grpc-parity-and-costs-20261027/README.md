# Message merging and gRPC costs, October 27

This record covers the runtime at `a24c0e3561c44c3ae03f57641e806ac7331b4df4`.
It contains correctness controls and local measurements, not a qualification
of overall performance. The endpoint comparison uses the previous frozen
`1d773f9787efd5dd389bb0f9896ba88c2eb1898e` implementation as its baseline.
The [preceding record](../grpc-encode-reuse-20261026/README.md) contains that
revision's source bundle and its comparison with `e7595eb7`.

| File | Contents |
|---|---|
| [results.json](results.json) | Summaries, source hashes, repeat values and limits |
| [raw.tar.gz](raw.tar.gz) | Commands, source patch, logs, fixtures and individual captures |
| [manifest.json](manifest.json) | Hash and size of every archived file |
| [check.py](check.py) | Verify the archive and results without extracting them |

Run `python3 docs/evidence/grpc-parity-and-costs-20261027/check.py` from the
repository root. The archive contains text and small wire fixtures, not
compiled executables. Its original paths start with `work/`; place them under
that directory to use the included reproduction notes and adjust absolute
paths to the checkout in use.

## Separate-process RPC comparisons

The release builds use Rust 1.99.1 on a shared Linux x86_64 host, plaintext
loopback, two Tokio workers per endpoint, one connection and one call in
flight. Both endpoints use Prost and check complete request and response
bodies. Each N/2N pair subtracts process startup from the per-RPC slope.
The allocation build has counters enabled; the CPU build is separate and has
counters and profilers disabled. The CPU clock is sampled in 10 ms ticks.

| Native endpoint and workload | Before | After | Paired change |
|---|---:|---:|---:|
| Client, 64 × 1 KiB identity bidi; requested bytes/RPC | 360,636 | 341,586 | −5.20% |
| Server, 64 × 1 KiB identity bidi; requested bytes/RPC | 332,756 | 343,527 | +3.79% |
| Client, four 1 MiB noise gzip bidi messages; MiB/RPC | 29.435 | 29.435 | approximately 0% |
| Server, four 1 MiB noise gzip bidi messages; MiB/RPC | 28.425 | 28.425 | approximately 0% |
| Server, 1 KiB identity unary; requested bytes/RPC | 15,059 | 14,438 | −4.12% |
| Server, 1 KiB identity unary; CPU µs/RPC | 46 | 38 | −13.04% |

Before and after values are medians; each paired change is the median of the
per-pair changes, so it need not equal the ratio of those two medians.
Allocation changes use two paired repeats. The 64-message client
repeats span −11.58% to +1.18%; server repeats span −2.27% to +9.86%. A fresh
tonic/prost control uses 21.72 MiB/client RPC and 20.71 MiB/server RPC for
the noise gzip cell, so the native stack still requests more in that workload.
The three unary-server CPU changes span −24.00% to −11.11%; its fresh tonic
control has a 30 µs/RPC median. Native-client unary CPU changes span −26.67%
to +18.18%. These short shared-host runs do not establish a latency or
throughput ranking. The 72 allocation captures contain 36,360 successful
RPCs; the 36 counters-off captures contain 270,000. Every captured call
completed successfully.

## Isolated costs and tradeoffs

These fixtures expose specific work and are not cross-library rankings.
Requested bytes measure allocator traffic, not peak memory.

| Fixture | Before | After | Scope |
|---|---:|---:|---|
| gRPC frame assembly, 64 × 1 KiB | 18,322 | 2,887 | Requested bytes per fixture; complete successor frames stay in their original buffer |
| DynamicMessage, repeated singular fragments | 142 allocations / 32,920 bytes | 61 / 14,488 | Parse one 5,490-byte fixture; both outputs are identical |
| MiniTable, 100 singular fragments | 306 allocations / 14,216 bytes | 4 / 154 | Parse one fixture with required child fields |
| Generator, 1,000 messages across 20 files | 709,890 allocations | 697,223 | Shared immutable type index; generated output is identical |
| Tower client-streaming construction | 4 allocations / 5,552 bytes | 3 / 2,304 | Construct and drop an unpolled call; matched features, debug build |

The MiniTable one-fragment fixture increased from 100.68 to 109.80 ns in
shared-host samples. Normalizing a map with 8,000 distinct keys added three
allocations and measured 16.87% more time than the old append-only path; the
old path did not replace duplicate keys correctly. The recorded MiniTable
candidate used the shared repository while other modules were changing; its
decoder hashes and a reconstruction using only the decoder patch are in the
archive. Generator warm-time medians varied by case, including a 16.81%
increase for the 20-file full-output case. Its single-file fixture requests
one extra allocation and 40 extra bytes. These results do not establish a
generator or codec speed advantage over prost.

The server's already-closed, deadline-free reset avoids two redundant state
locks in the measured lifecycle, and the private unary read future shrinks
from 640 to 24 bytes. Those are structural controls, not a measurement of the
public future or an independent endpoint speed claim. Tower's direct input
keeps the existing bidi upload task and removes the extra forwarding task and
queue; live streams still need an explicit replay policy for retries.

## Correctness checks

- The all-feature workspace run passed 3,902 tests, with nine existing ignored
  tests. It began at `346b967d`; while it compiled, a test-only empty-output
  assertion was changed for all-feature type inference and committed as
  `a24c0e35`. The affected test binary was built after that change; production
  sources are identical between the two commits. One ignored pinned-descriptor
  regeneration test was run separately with protoc 35.1 and passed.
- Strict core all-target Clippy, transport library Clippy, warning-denied
  workspace documentation, the RPC and codec harness tests, and the Python
  accounting checks passed. Five core/native/Tower library profiles passed on
  Rust 1.85 at `a24c0e35`.
- The pinned protobuf v35.1 conformance runner passed two required runs and
  one recommended run, each with 5,631 binary/JSON and 909 text cases, with
  no skips or failures. Its maximum tested edition is 2023. All 13 bundled
  bindings also matched regeneration with the pinned compiler.
- New controls cover split required children, oneof and map replacement,
  recursion limits, malformed wire input, MessageSet first-field semantics,
  Tower backpressure and cancellation, and a frame followed by a complete
  zero-copy successor. Several controls were run against the old source to
  demonstrate that they detect the original defects.

The first allocation-build attempt was rejected because the source commit
changed during its build; its logs are retained. The later source-pinned build
and all captures above completed successfully.
