# Early unary filter selection, November 10

An **unmerged** filter-selection draft moved the method-filter check before
a unary call handle is returned. It avoids the logging-capable cancellation
path for excluded methods. In this screen, all five warmed allocation samples
requested eight more bytes per RPC in no-logger and excluded modes. The source
of that increase was not established by this screen. Keep this result when
evaluating later revisions.

| Logger | Workload | Draft / 1e: median (range) | Draft / prior draft: median (range) |
|---|---|---:|---:|
| No logger | Successful unary | 1.0084 (0.9672–1.0281) | 0.9888 (0.9686–1.0115) |
| No logger | Cancellation | 0.9978 (0.9655–1.0161) | 0.9914 (0.9682–1.0206) |
| No logger | Held construction | 1.0031 (0.9857–1.0331) | 1.0046 (0.9742–1.0233) |
| No logger | Construct and drop | 0.9199 (0.6297–1.0796) | 0.9446 (0.8093–1.0763) |
| Excluded | Successful unary | 1.0053 (0.9718–1.0172) | 0.9890 (0.9572–1.0120) |
| Excluded | Cancellation | 1.0059 (0.9659–1.0287) | 0.9938 (0.9675–1.0121) |
| Excluded | Held construction | 1.0310 (1.0011–1.0494) | 1.0184 (1.0091–1.0417) |
| Excluded | Construct and drop | 1.1937 (1.0545–1.3456) | 1.1374 (1.0012–1.2724) |
| Enabled | Successful unary | 1.0145 (0.9788–1.0340) | 0.9959 (0.9509–1.0200) |
| Enabled | Cancellation | 1.0088 (0.9892–1.0203) | 0.9978 (0.9740–1.0213) |
| Enabled | Held construction | 1.0182 (0.9919–1.0324) | 1.0183 (1.0012–1.0250) |
| Enabled | Construct and drop | 1.1494 (0.9199–1.3833) | 1.0265 (0.8266–1.2602) |

Lower is better. All ratios are process CPU times from ten alternating
ABCCBA/CBAABC blocks; each block also included same-binary controls. The
ranges are observed values, not confidence intervals. The short construct/drop
measurements and several endpoint controls varied widely. These results compare
revisions of pbrs-grpc; they are not a tonic comparison.

The [screen](https://github.com/mingley/pure-protobuf/actions/runs/42732002318)
compared baseline [`1e6174f2`](https://github.com/mingley/pure-protobuf/commit/1e6174f2c6463f29488bcfbd432525e0cd9f94f1),
prior unary-log draft [`9449f8e8`](https://github.com/mingley/pure-protobuf/commit/9449f8e8cabd05451edf460a0e30a9a73097ef29),
and early-filter draft [`a0ba5497`](https://github.com/mingley/pure-protobuf/commit/a0ba54970b38d1157fbefaf98613c1d6ad84eb5b).
It used Rust 1.99.1 release builds on a shared AMD EPYC 7763 runner, pinned to
one logical CPU. Each process warmed 50 calls. Endpoint runs used 5,000
successful calls or 1,000 cancellations with a native client and server in
one current-thread Tokio process over loopback TCP without TLS. Each successful
reply was checked; cancellation included per-call mpsc entry/drop
coordination. Timed binaries did not include explicit handler-count,
allocation-count or copy-count instrumentation; runtime internals can use
atomics.

The excluded mode configured a logger whose filter excluded the method and
checked that the sink stayed empty. The enabled timed mode used a no-op sink;
separate smoke runs checked that records were emitted. Held construction
retained 5,000 unpolled calls until after timing, so cancellation allocations
could not be reused. Construct/drop timed each unpolled call's creation and
drop, including cancellation. Neither construction case measures actual RPCs.

Separate allocation-instrumented endpoint binaries measured five alternating
N=250 and 2N=500 samples per variant and configuration. In both no-logger
and excluded modes, successful calls requested 11.008 allocations and zero
reallocations per added call across all variants, but requested bytes rose
from 15,962.928 to 15,970.928 in the early-filter draft. Cancellation
requested 26 allocations and zero reallocations; bytes rose from 18,562 to
18,570. Enabled samples had the same allocation/reallocation counts and
roughly eight additional requested bytes in the draft, with small byte-count
variation. The counts cover both endpoints and warmed spares, not cold-call
allocation or peak memory.

The [retained data](unary-filter-refinement-screen-20261110.json) includes
every timing block, same-binary control, allocation sample, source and binary
hash. The original [artifact](https://github.com/mingley/pure-protobuf/actions/runs/42732002318/artifacts/13726585026)
has SHA-256 `0485b3d16388879377ce1000c5c3589819f9f1c951790a6bdaacc0217fb04b17`
and expires November 24, 2026. Earlier related results are in the
[integrated cancellation record](integrated-unary-cancel-screen-20261109.md).
