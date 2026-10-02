# SB-21 aggregate-arrival/accounting diagnostic — 2026-10-02

Source `1e6b1119182e1e4e90f2d93656765b3bf4929b81` (after `3f1ec668`)
implements the benchmark-only reference arrival/accounting overlay. This completes
the code gap; SB-21 remains open for valid headroom and broader qualification.
No benchmark threshold, shipping transport/codec, dependency lockfile or wire
policy changed. The original failed runs remain intact.

The [machine summary](sb21-arrival-accounting-20261002/diagnostic-summary.json)
links every measured row. [Artifact hashes](sb21-arrival-accounting-20261002/artifact-sha256.json)
cover the raw results, worker logs, plans, frozen stack report and gate logs.
The [Go overlay manifest](sb21-arrival-accounting-20261002/go-overlay-manifest.json)
records exact original module/client, helper, patched client, dependency and
binary hashes. Original Go server and closed-loop paths stay unmodified upstream.

## Functional proof and pins

- Rust 1.99.0, genuine protoc 35.1, Go 1.25.3; SDK archive provenance is retained.
- Native tests: 94 binary + 96 worker + 1 fairness = 191 passed. Four focused
  Go tests passed, including the independently derived shared schedule vector,
  reset/carry-in/failure conservation, real bounded-slot rejection/cancellation
  and new-stream one-message/one-reply byte counts.
- Python: 16 QPS and 37 stack tests passed; two stack credential tests skipped.
- Native executable built at the clean source pin above, jobs=1. Native debug
  SHA-256: `87dd1cfb477ede5e7399b67937c89904670a684b5c0734f59337fcb5e00fdfb3`.
- Overlaid Go SHA-256: `bd863f87f4a48dd41a94073e5ffd4b0d2cae06b071522859c3c137b4296e362a`;
  original Go SHA-256: `cf7492e173a815fce2857faf4b8f330ed8e24d1e141313530a6a8b0afcd567af`;
  integrated driver SHA-256: `0a3781d54a2681670f07a20d28b0b6d5e5f3439dcfa20171cc3dd5fc0cc20577`.
- Strict Clippy did **not** pass: the all-target attempt first found two existing
  `load_shapes.rs` lints; narrowed worker and binary runs exposed 37 and 47
  existing lints respectively. Complete failures are retained without waiver.
- The first Go dependency fetch was denied at a `storage.googleapis.com` redirect.
  The coordinator-approved, already-permitted `goproxy.io` route fetched the same
  locked dependency with normal `go.sum` verification. Both attempt logs remain.

## Quiet diagnostic outcomes

The coordinated lease lasted 141.871 seconds, including startup/control and
between-command inspection. This exceeded the estimated 75 seconds; the actual
duration is retained. The host had a four-CPU cgroup quota and five-CPU inherited
affinity. These were loopback diagnostics with **debug native versus optimized Go**.
They establish accounting behavior, not performance or headroom qualification.

Two independently randomized repeats ran all three native/mixed unary directions
at the frozen 5,000 aggregate QPS, with 1-second warmup and 2-second measurement.
A Go/Go pair and a Go-to-native 1 KiB streaming cell also ran. All eight reconcile
offered, dispatch, completion, rejection, timeout, unfinished and carry-in counts,
both latency histograms, driver windows and effective scenario metadata.

| Cell | Repeat | Offered | Dispatched | Completed | Rejected | Unfinished | Completion QPS |
|---|---:|---:|---:|---:|---:|---:|---:|
| native → native unary | 1 | 10182 | 10091 | 10102 | 91 | 17 | 5029.4 |
| native → Go unary | 1 | 10191 | 9828 | 9822 | 363 | 46 | 4887.5 |
| Go → native unary | 1 | 10148 | 9849 | 9838 | 299 | 27 | 4916.8 |
| native → native unary | 2 | 10184 | 9993 | 10027 | 191 | 18 | 4992.6 |
| native → Go unary | 2 | 10194 | 9701 | 9728 | 493 | 27 | 4842.3 |
| Go → native unary | 2 | 10157 | 9916 | 9922 | 241 | 36 | 4954.9 |
| Go → Go unary | 1 | 10149 | 10149 | 10140 | 0 | 12 | 5067.5 |
| Go → native streaming, 9,000 aggregate QPS | 1 | 17965 | 7887 | 7888 | 10078 | 63 | 3942.1 |

Completions can exceed this window's dispatches because carry-in is explicit;
all window equations reconcile. There were no timed-out/failed dispatched calls
in these cells. Native/mixed unary CPU was near a full core on one endpoint;
91–493 rejections remained visible. Streaming retained all 10,078 admission
rejections. `PASS` in the runner is a driver/accounting preflight status; every
proof and summary still has `claim_eligible=false`.

The current unmodified pinned Go client completed 4,528.06 QPS against the debug
native server. This run did **not** reproduce the historical >5,000 completed-QPS
outlier. Its source still configures 64 independent chains at 5,000 per-slot QPS
(320,000 nominal aggregate); observed completions cannot reconstruct offered
arrivals. Diagnostic validation correctly remains unverified, and explicit
`--claims` rejects the missing independent counters/scheduled-send latency.
The historical 11,058.2-QPS artifact and later failed original-Go run stay unchanged.

The frozen two-cell stack selection preserved all five randomized repeats,
15/60-second original configuration and explicit 1/2-second smoke overrides.
All ten rows have reconciled counters and service/scheduled histogram sample
counts, and **all ten remain INVALID**. Scheduling-lag p50 was 55.8–63.8% of
end-to-end p50 against the unchanged 10% limit. The harness returned zero because
it completed diagnostic execution; `selection_complete=false` and
`matrix_complete=false` preserve the failed qualification.

The stack monitor also divided CPU utilization by observed thread count (two)
despite a one-CPU enforced pin. Its printed 82–90% spare-capacity values cannot
qualify headroom. This denominator defect is retained for a separate bounded
SB-24 correction using enforced affinity and cgroup quota. Server headroom was
not certified after the earlier scheduling-lag failure.

## Optimized follow-up

A native release build completed at the same clean source pin in 5m27s, with total
owned debug+release target about 1.5 GiB (below the 2 GiB budget). Release SHA-256:
`7fdbe0be0ddf38280706cb4eda5f43915d2381c4338aaf844092357d2a3b6ff8`.
An optimized 5,000-QPS reproduction is pending its own quiet lease. It must retain
the same offered rate, gate thresholds and old-versus-corrected denominator label;
the current denominator cannot establish headroom. Dedicated hosts, complete peer
coverage, aligned windows, saturation/headroom proof and contract sample/confidence
requirements remain outstanding.
