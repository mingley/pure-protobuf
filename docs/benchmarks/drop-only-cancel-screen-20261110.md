# Drop-only cancellation screen

This candidate avoids initializing logging state when an unfinished `Call` is
dropped and no unary logger has registered. The cancellation flag is still set
and waiters are woken. If a logger is registered, the existing logging-aware
path runs. Explicit cancellation, including through a detached handle, retains
that path because the call could still be polled and register later.

The shortcut depends on the owning unary future being the only production
registrar and registering before it spawns hedges. After `Call::drop` begins,
that future cannot be polled again. A future streaming observer or child task
that registers later needs a new handoff or removal of this shortcut. These
measurements do not establish tonic or Prost wins, general feature parity, or a
production ranking.

## Source and method

The [first screen](https://github.com/mingley/pure-protobuf/actions/runs/42750683260)
compared exact source `09d8ad6b0b1ee4fec15fe14f2521d0031fa41997` with
prototype `5b749b138e09bb5ea5a3d94aa8a40e76ee729b0a`. The
[longer endpoint repeat](https://github.com/mingley/pure-protobuf/actions/runs/42752378525)
used the same source and release binary hashes. The change is being integrated
on a later documentation-only parent; these are historical source measurements,
not a measurement of a later commit. [Raw pairs and allocation samples](drop-only-cancel-screen-20261110.json)
include fixture and binary hashes.

Both screens use shared GitHub runners, one current-thread native client and
server over loopback TCP without TLS, process CPU, 50 warmups, alternating
ABBA/BAAB blocks and paired same-binary controls. Timed binaries omit explicit
handler, allocation and copy counters; runtime atomics remain. Success validates
replies. Cancellation includes mpsc entry/drop coordination and checks inside
the timed interval. Enabled logging uses a no-op sink; separate smoke checks
verify records, and excluded sinks are checked empty. These samples are not a
general production workload. Ratios below are candidate divided by baseline,
so lower is faster. Ranges are observed block ratios, not confidence intervals.

## Unpolled construction and drop

The first screen used an Intel Xeon Platinum 8370C, 5,000 operations per sample
and ten blocks. Held calls are retained until after timing; lifecycle calls are
constructed and dropped in the timed loop.

| Mode and case | Median | Block range | Slower blocks | Baseline control range | Candidate control range |
|---|---:|---:|---:|---:|---:|
| No logger, held | 0.9987 | 0.9623–1.0090 | 3/10 | 0.9139–1.0281 | 0.9850–1.0132 |
| No logger, lifecycle | 0.9941 | 0.9169–1.1148 | 4/10 | 0.9777–1.2234 | 0.7989–1.2424 |
| Excluded, held | 1.0022 | 0.9827–1.0184 | 7/10 | 0.9757–1.0351 | 0.9886–1.0350 |
| Excluded, lifecycle | 0.9218 | 0.8298–1.0265 | 2/10 | 0.8442–1.1839 | 0.8953–1.1567 |
| Enabled, held | 0.9952 | 0.9705–1.0164 | 3/10 | 0.9810–1.0308 | 0.9770–1.0222 |
| Enabled, lifecycle | 0.9092 | 0.8784–0.9433 | 0/10 | 0.9119–1.1607 | 0.8415–1.2360 |

The configured-logger lifecycle medians improved on this runner, but the
control ranges are wide. Held construction provides a separate control and
does not measure drop. The [longer construction/drop repeat](drop-only-construction-repeat-20261110.md)
used exact 452-to-41d sources.

## Endpoint repeat

The longer repeat used an AMD EPYC 7763, 10,000 RPCs per sample and twenty
blocks; individual sample CPU was 0.310–0.499 seconds.

| Mode and case | Median | Block range | Slower blocks | Baseline control range | Candidate control range |
|---|---:|---:|---:|---:|---:|
| No logger, success | 0.9987 | 0.9683–1.0172 | 8/20 | 0.9602–1.0471 | 0.9769–1.0502 |
| No logger, cancel | 1.0003 | 0.9774–1.0332 | 12/20 | 0.9599–1.0477 | 0.9590–1.0266 |
| Excluded, success | 1.0006 | 0.9804–1.0205 | 10/20 | 0.9740–1.0497 | 0.9793–1.0409 |
| Excluded, cancel | 0.9957 | 0.9630–1.0177 | 6/20 | 0.9613–1.0453 | 0.9822–1.0331 |
| Enabled, success | 0.9944 | 0.9748–1.0216 | 8/20 | 0.9660–1.0355 | 0.9836–1.0200 |
| Enabled, cancel | 1.0087 | 0.9829–1.0408 | 14/20 | 0.9627–1.0464 | 0.9682–1.0249 |

An earlier screen's no-logger cancellation was slower in 10/10 short blocks
(median 1.0029); the longer repeat does not reproduce that direction. Its
enabled-cancellation median was 1.0087, within the observed control ranges.
The screens do not establish a general endpoint speedup or rule out small
costs under other conditions.

## Layout and allocation

In separate instrumented builds on the first runner, `Call`, the unary future
and cancellation `Inner` measured 32, 5,632 and 80 bytes in both variants.
A retained, unpolled call requested two allocations and 5,728 bytes in both
variants. This measures allocation requests before drop, not peak memory or
drop-time allocation. The earlier cancellation fix enlarged `Inner` by
8 bytes and a retained call by 8 requested bytes versus its older baseline;
this change does not remove that cost.

Warmed process-wide N/2N endpoint measurements showed matching allocation and
reallocation counts in all measured modes. No-logger and excluded requests
matched exactly in requested bytes; enabled bytes varied slightly between
samples. Allocation and layout builds are separate from timed binaries.

## Correctness scope

The [diagnostic](https://github.com/mingley/pure-protobuf/actions/runs/42751263903)
covered all four call shapes, detached and early explicit cancellation, a
registered logger, active wake and primary-waker panic, concurrent signals,
and reuse of a logged spare. It used assertion-specific negative controls and
Rust 1.85. Unary cancellations before logger registration can still be omitted;
streaming can still misattribute peer `CANCELLED`; retry/hedge logs can include
uncommitted attempts; early upload rejection trailers can be omitted. See the
[unary cancellation record](../evidence/unary-binlog-cancellation-20261110.md)
for the broader limits.
