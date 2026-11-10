# Drop-only construction repeat

This is a follow-up to the [drop-only cancellation screen](drop-only-cancel-screen-20261110.md).
It compares exact parent `452d2d52c005d09d44c6531a245542b5623d68e9`
with candidate `41d94325b8aa48b10b31fb6709c1a41dbc44a72a`.
It measures unpolled call construction and drop, not completed RPCs or tonic.
The candidate skips logging-state initialization on drop when no unary logger
has registered; cancellation and waiter wakeup remain active.

The [run](https://github.com/mingley/pure-protobuf/actions/runs/42754167155)
used a shared AMD EPYC 9V45 runner and a pinned logical CPU. A process warmed
50 calls before timing. Lifecycle samples constructed and dropped one million
calls; held samples retained 25,000 calls until after timing. Retention prevents
reuse of their cancellation allocations and can use substantial memory. Each
case also had a separate untimed warmup process. No explicit allocation or copy
counters were enabled. A configured logger used a no-op sink; unpolled calls do
not create call-log records. All [raw pairs](drop-only-construction-repeat-20261110.json)
and binary, fixture, and source hashes are retained.

Twenty blocks alternate ABBA and BAAB ordering; each block also includes
same-binary controls. The ratio is candidate CPU divided by baseline CPU.
Ranges show observed block ratios, not confidence intervals. No outliers were
removed. Timed pair samples used 0.159–0.260 seconds of process CPU for
lifecycle and 0.046–0.111 seconds for held construction.

| Mode and case | Median | Block range | Slower blocks | Baseline control range | Candidate control range |
|---|---:|---:|---:|---:|---:|
| No logger, held | 1.0075 | 0.9575–1.4701 | 15/20 | 0.9663–1.0344 | 0.8382–1.0389 |
| No logger, lifecycle | 1.0024 | 0.8768–1.0365 | 14/20 | 0.7065–1.0326 | 0.9604–1.3019 |
| Excluded, held | 1.0036 | 0.9725–1.6055 | 14/20 | 0.9490–1.0530 | 0.9697–1.0355 |
| Excluded, lifecycle | 0.9400 | 0.9091–0.9625 | 0/20 | 0.8402–1.0580 | 0.8470–1.1468 |
| Enabled, held | 1.0036 | 0.9524–1.0447 | 13/20 | 0.9538–1.0361 | 0.9482–1.8932 |
| Enabled, lifecycle | 0.9368 | 0.8353–1.0488 | 1/20 | 0.9602–1.1678 | 0.9715–1.1634 |

The configured-logger lifecycle gain repeated on this runner. The no-logger
lifecycle and all held medians overlap observed control ranges; their majority
slower direction and the large held/control outliers merit care. These tests
do not prove no regression on other workloads or machines. Previous
[endpoint results](drop-only-cancel-screen-20261110.md#endpoint-repeat) include
successful and cancelled loopback calls and show their own limits. The
allocation and +8-byte cold-state costs from the existing unary logging fix
also remain; the earlier record separates allocation and timing builds.

The artifact's ZIP SHA-256 is
`5885c830c88437b03f26e74537cbebecde4b3af7316714a66411ae069d38c451`
and its original Actions retention was 14 days. Source, fixture, binary hashes,
and all individual samples are in the checked-in JSON. This result alone
cannot establish full feature parity, broad production performance, or a world
ranking.
