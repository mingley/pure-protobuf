# Qualification history

No performance category has passed qualification yet. The project has useful
local results and working benchmark tools; it has not completed the controlled
campaigns needed for repeatable comparisons across the required workloads.

The [execution follow-up](../evidence/grpc-architecture-20261022/README.md) adds
wake-indexed scheduling, runtime-local cancellation parking, selected-method
prost dispatch, complete compressed-message
handling, tonic gzip normalization and a separate 1,024-member/frame default
limit. Runtime `9d6fb9c4` also fixes blocked duplex uploads, task-abort cancellation,
waiter-slot lifetime and transport ownership. Its leak-detecting sanitizer run
passes **731 tests**, with one existing ignore, and all ten pre-workspace checks
pass. The clean `deb0d1d6` all-feature workspace passes **3,755 tests**, with zero
failures and nine existing ignores across 139 targets. Only the serving fixture
differs from the runtime source; sanitizer and binary proofs keep their own pin.
The original C8/C3 workspace failures remain archived. The actual current resource
preview passes 30.54 seconds and nine drains, all with client/server byte charges,
tokens and active calls at zero. The actual 24-hour run started at 02:52 UTC
on 2026-10-23; its result is pending. No performance standing is established.

Current benchmark captures use matched 400 KiB server send buffers and request
queues of 16 at `2eba72d8`/`9d6fb9c4`. Historical full-matrix counts are not the
source-paired baseline for this harness. Source comparisons cover selected
workloads; the full current matrix compares native with tonic. The expanded
campaign passes 2,400 captures and 720,000 RPCs. Selected source pairs pass 400
captures and 120,000 RPCs; 1 MiB pairs pass 120 captures and 4,500 RPCs. Gzip byte
improvements, identity bidi losses and source regressions remain; 1 MiB gzip bidi
still requests 71–76% more bytes than tonic/prost. The focused
server-instruction diagnostic passes 72 captures and 1,080 RPCs; with pilots,
the combined campaigns pass 3,000 captures and 845,790 RPCs. These instrumented
results do not qualify shipping CPU, latency or throughput.

The `6cd91911` [day attempt](../evidence/grpc-streaming-soak-20261022/README.md)
failed after 32 min 2.73 s with an unexpected INTERNAL on TCP bidi reset.
The exact failure was not uniquely reproduced in 2,008 scenario replays.
Focused reset-ordering controls are separate evidence; no full day is proved.

The [earlier streaming comparison](../evidence/grpc-streaming-20261022/README.md)
retains its 2,400-capture native campaigns and 144-capture instruction pilot.
It reports selected 1 KiB allocation savings alongside 64 KiB gzip byte losses
and instruction regressions. Its values belong to those source pins. Native CPU,
latency, throughput and scheduling qualification remain open, as do required
feature/platform/release gates. No qualification standing changes.

The table began with the 2026-09-29 audit; later source-scoped records above
update specific findings. It remains a manual index until SB-16 supplies the
generated scoreboard and does not certify the latest GitHub Actions run.

The [2026-10-08 gRPC record](../evidence/grpc-readiness-20261008/README.md) adds
separate-process functional coverage and per-side N/2N counter measurements.
It retains streaming instruction/byte losses, a failed counter capture,
intermittent fairness failures, and a 24-hour attempt that stopped after
504 seconds. These diagnostics do not change the qualification standings below.

The machine-readable source is
[bench/scoreboard/categories.json](../../bench/scoreboard/categories.json). It
defines 52 categories, one owner lane per category, metric direction,
comparator pins, and corpora. The
[strategy](../plan/world-class/README.md#scoreboard) defines the groups, and the
[benchmark contract](../benchmark-contract.md) defines evidence tiers. This page
records the current evidence available for each category. Start with
[benchmarks.md](../benchmarks.md) for the findings and
[the evidence index](../evidence/README.md) for individual investigations.

`unmeasured` below means **no qualified comparative result**, even when a local
diagnostic exists. An `invalid` historical measurement cannot support a ranking.
For performance categories, `win`, `tie`, or `loss` requires contract §6–§10
evidence. Correctness gates are listed separately: implemented checks and old
passing reports do not establish that an arbitrary current revision is green.
