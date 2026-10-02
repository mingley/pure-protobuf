# Endpoint headroom acceptance audit — 2026-10-02

This read-only audit uses main source
`796005d373ddef825a9e185f433c7f879a5ce51b`. It reopens unproved acceptance
requirements while retaining delivered harnesses, attribution and historical
diagnostics. It changes no benchmark thresholds or historical result bytes.

| Card | Delivered work | Remaining qualification |
| --- | --- | --- |
| SB-11 | Cross-stack harness and diagnostic smoke | Required peers/cells and verified endpoint headroom |
| CL-01 | Client cost attribution and optimization hypotheses | Verified reference-server headroom for every cell |
| SV-01 | Server cost attribution and optimization hypotheses | Verified generator headroom for every cell |
| BM-06 | Separate endpoint CPU/RSS counters and reporting | Effective capacity and aligned measurement-window proof |

The retained [SB-11 smoke report](stack-matrix-smoke-report.json), SHA256
`30b585db971d618a45b9c41aaa1d554261f9f26c30c4a6d2948d9d6cd71e23ae`,
sets `headroom.ok=true` on the native and tonic client empty/plain/one-CPU
rows while both have `pinned=false`. Pinning is unsupported on that Darwin
host; neither row contains effective CPU-capacity or endpoint counter-window
proof. The accompanying [evidence prose](stack-matrix-sb11.md) already limits
the run to diagnostic smoke. Those two numeric certifications cannot close
the card's verified-headroom acceptance.

[Client profiles](client-profiles.md) report client saturation checks passing;
the card requires server headroom for every cell. Its cited source
`7aec79e22914cce4cd280f886ced4dae892d169f`,
`scripts/rpc-bench-matrix.py` lines 340–351, uses an unweighted sample mean
and observed client thread count as a denominator. It supplies no verified
server-capacity assurance. This does not invalidate exact codec allocation
attribution.

[Server profiles](server-profiles.md) retain eight two-second diagnostic rows
without generator CPU-capacity/window proof. Their documented schedule-p50
to end-to-end-p50 ratios range from 49.6% to 75.8%, exceeding the unchanged
10% scheduling-lag qualification guard. The missing generator proof is an
evidence gap; this audit does not invent a numeric generator CPU result.

BM-06's retained evidence states that the crate builds. At the audited source,
`rpc-bench/src/resources.rs::detect_cgroup_quota_millicpus` reads hardcoded
mount-root files rather than each PID's cgroup membership, ancestor quota
minima and namespace visibility. Its `CpuConstraints` data remains diagnostic.
The separate [Python capacity correction](sb24-cpu-capacity-20261002.md) and
[counter-window correction](sb24-counter-window-20261002.md) supply bounded
guard implementation and regression evidence. The actual session host still
fails capacity verification; neither correction constitutes a dedicated-host
headroom campaign. Alignment with the RPC measurement window also remains open.

All original evidence stays intact. The world-class SB-11, CL-01 and SV-01
cards and legacy BM-06 card return to `in_progress` for these explicit
remaining acceptance requirements.
