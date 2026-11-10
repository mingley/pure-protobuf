# No-capture unary filter experiment, November 10

An **unmerged** early-filter refinement read cancellation capability from
the existing handle to avoid increasing unary future size. It still moved
filter resolution into call construction. Excluded held-call construction
was slower than the unrefined draft in all ten blocks, while endpoint results
did not show a decisive win. The simpler unary-log draft was selected for the
current candidate. These shared-host observations do not establish an overall
ranking or a precise production cost.

| Logger | Workload | Refinement / 1e: median (range) | Refinement / prior draft: median (range) |
|---|---|---:|---:|
| No logger | Successful unary | 1.0094 (0.9891–1.0469) | 0.9960 (0.9787–1.0415) |
| No logger | Cancellation | 1.0012 (0.9934–1.0194) | 1.0047 (0.9787–1.0197) |
| No logger | Held construction | 1.0058 (0.9856–1.0256) | 1.0087 (0.9733–1.0194) |
| No logger | Construct and drop | 0.9502 (0.8499–1.1431) | 1.0048 (0.8950–1.1720) |
| Excluded | Successful unary | 1.0075 (0.9955–1.0305) | 1.0042 (0.9979–1.0110) |
| Excluded | Cancellation | 1.0081 (1.0002–1.0331) | 0.9981 (0.9692–1.0144) |
| Excluded | Held construction | 1.0389 (0.9840–1.1147) | 1.0352 (1.0197–1.0991) |
| Excluded | Construct and drop | 1.1242 (0.1938–1.2587) | 1.0947 (0.8245–1.1783) |
| Enabled | Successful unary | 1.0238 (0.9974–1.0560) | 0.9970 (0.9694–1.0272) |
| Enabled | Cancellation | 1.0168 (0.9886–1.0555) | 1.0004 (0.9761–1.0191) |
| Enabled | Held construction | 1.0248 (0.9583–1.0654) | 1.0154 (0.9933–1.0478) |
| Enabled | Construct and drop | 1.1757 (1.0175–1.3262) | 1.0803 (0.9910–1.2386) |

Lower is better. Each ratio compares sums from ten alternating
ABCCBA/CBAABC blocks. Same-binary controls are retained with every block.
Several short construct/drop controls varied widely, including 10× outliers;
do not interpret those rows as precise cost estimates. The excluded held-call
refinement/prior ratio was 1.0352 (1.0197–1.0991), slower in all ten blocks.

The [screen](https://github.com/mingley/pure-protobuf/actions/runs/42735795817)
compared [`1e6174f2`](https://github.com/mingley/pure-protobuf/commit/1e6174f2c6463f29488bcfbd432525e0cd9f94f1)
with the [unrefined unary-log draft](https://github.com/mingley/pure-protobuf/commit/9449f8e8cabd05451edf460a0e30a9a73097ef29)
and the [no-capture refinement](https://github.com/mingley/pure-protobuf/commit/ecc5c436a9c3c151923cc3ce5b2e81ef27555b43).
It verified the runtime source at `6560281b` was unchanged from 1e, but does
not measure later source changes. Rust 1.99.1 release builds ran on a shared
AMD EPYC 7763 pinned to one logical CPU. Every process warmed 50 calls.

Endpoints used 5,000 successful RPCs or 1,000 cancellations in one
current-thread Tokio process with native client and server over loopback TCP,
without TLS. Successful replies were checked, and cancellation included
per-call mpsc entry/drop coordination. Timed binaries omitted explicit
handler-count, allocation-count and copy-count instrumentation; runtime
internals may use atomics. Excluded mode configured a logger but checked its
sink stayed empty. Enabled timed runs used a no-op sink; separate smoke runs
checked that records were emitted.

Held construction retained 5,000 unpolled calls until after timing, preventing
spare reuse. Construct/drop timed each unpolled call's creation and drop,
including cancellation. Neither measures an actual RPC. Separate allocation
binaries found equal warmed N/2N requests for baseline, prior and refinement
in no-logger and excluded modes: 11.008 allocations, zero reallocations and
15,962.928 requested bytes per successful call; 26, zero and 18,562 per
cancellation. Enabled samples had equal allocation/reallocation counts with
small requested-byte variation. These deltas are not cold or peak memory;
the [layout record](unary-filter-layout-20261110.md) includes a separate
retained-call cold allocation check.

The [data](unary-filter-no-capture-screen-20261110.json) retains every block,
same-binary control, allocation pair, source and binary hash. The original
[artifact](https://github.com/mingley/pure-protobuf/actions/runs/42735795817/artifacts/13728185506)
has SHA-256 `03e7704d7754002542a72dd33e7259ee44142faa31ec64aa5918478a4ab0f798`
and expires November 24, 2026. This is not a tonic comparison.
