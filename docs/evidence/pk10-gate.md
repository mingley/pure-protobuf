# PK-10 gate: Phase 3 precondition fails — stop with evidence

PK-10 ("Parse segmented DATA frames without coalescing", zero-copy plan
Phase 3) has a measurement precondition: *after 1a, 1b and 3-alt, profiling
still attributes at least 5% of server CPU per GiB to the `carry` copy, on LP
upload with 16 KiB peer frames*
(`docs/plan/pbrs-zero-copy-large-payloads-plan.md`, Phase 3).
It does not. Per world-class worker-protocol rule 2 (report the evidence
and stop; a disproved hypothesis is a useful result), Phase 3 is not started
and PK-10 is closed as superseded-by-measurement.

## Verdict

| Scenario | Carry share of busy server CPU | Gate |
|---|---|---|
| LP upload 8x1MiB, 16KiB peer frames (the gate scenario) | **1.9%** (172/9230 leaf samples; memmove-via-`FrameReader::push` 154 + realloc/reserve ~18) | need >= 5%: **FAIL** |
| Unary 8MiB, 1MiB native frames (cross-check) | **2.8%** (282/10104) | **FAIL** |

Both readings fail under every accounting tried: excluding only parked
threads, or additionally excluding `kevent` io-driver idle; counting only
`memmove` under `FrameReader::push` or every copy-candidate self sample.
Including idle CPU fails even harder (0.2%). The margin is >= 2x.
Even Amdahl-capped, deleting the carry copy could buy at most ~2% server
throughput in the most favorable scenario measured.

Where the fragmentation tax actually goes at 16KiB frames: `recvfrom`
syscalls are 20.4% of busy server CPU (1879/9230) — 64x the read rate of
1MiB framing — plus h2 event bookkeeping. Segmented parsing would not
touch any of that.

## What was measured (and why 3-alt does not obsolete the question first)

1. **Peer chunk matrix (1MiB unary, `PBRS_LOG_CHUNKS` probe at
   `FrameReader::push`).** Native and tonic peers deliver a 1MiB payload in
   one ~1MiB read plus 1-2 small reads (gRPC header bytes split by TCP
   segmentation); grpc-go sends 16KiB DATA frames (prior: 5393x16384).
2. **3-alt (scratch `DEFAULT_MAX_FRAME_SIZE` ~= 16MiB, uncommitted).**
   Mechanics work: 8MiB unary arrives in single ~8MiB reads, and tonic
   honors the larger advertisement (single 8MiB DATA frames). But
   throughput is unchanged-to-worse: base/base 117.7/113.5 qps vs
   3alt/3alt 110.2/109.4 qps (repeats; evidence rerun 113.5 vs 104.6),
   mixed pairs interop cleanly at ~115-117 qps. Frame count is not the
   lever: the header/payload TCP split puts *every* large message on the
   carry path regardless of DATA frame size, so 3-alt cannot reach the
   plan's "most inbound cells to zero copies" outcome either.
3. **Gate profile.** `sample` (macOS, ~700Hz x 15 threads) on the server
   during LP upload with a 16KiB-advertise server (go-sim: peer sends
   16KiB DATA frames; verified 141,952x16384 reads, 0 large reads).
   Self-CPU aggregated from call-graph leaves (box-drawing parser; all
   leaves classified, `???`/unmatched quantified at <0.2% busy).

Method trap caught and fixed: the first 3-alt matrix showed a fake 2x win
because 4800+ probe log lines filled the 64KiB captured-stdout pipe and
stalled the server. Caught by the asymmetry (only base/base slow),
fixed with file logging, and the gate profile was re-run with a
probe-free binary after the probe's `eprintln`+`getenv`-per-push showed
up as the top busy symbol (1034 samples) in the polluted capture.

## Artifacts

- `docs/evidence/pk10-gate/server-upload-16k.sample.txt` — raw `sample`
  capture, gate scenario (clean 16K-advertise server, 1224x8MiB upload,
  0 failures, p50 32.4ms; evidence rerun 1138 calls in `load-upload-16k.json`)
- `docs/evidence/pk10-gate/server-upload-16k.leaf.txt` — leaf self-sample
  table; `.memmove.txt` — memmove leaves by caller (154 carry / 23 h2 / 1 parse)
- `docs/evidence/pk10-gate/server-unary-8m.*` — unary 8MiB cross-check
  capture + leaf table (925 calls, 115.4 qps, 0 failures)
- `docs/evidence/pk10-gate/load-unary-8m-{base,3alt}.json` — 3-alt rerun
  raw reports

Scratch binaries, drivers and parsers lived in `/tmp` only (`/tmp/3alt.py`,
`/tmp/gate16.py`, `/tmp/leaf*.py`, `/tmp/mm*.py`, `/tmp/lp-baseline/`);
the worktree diff for the whole premise was a 4-line TEMP probe plus
TEMP constants, all reverted — `git status` clean before this commit
except the evidence files.

## Cabinet for the future

Re-open Phase 3 only if a new measurement passes the gate, e.g.: small
messages at high concurrency (per-message overhead regime differs);
non-loopback transports where TCP segmentation multiplies small reads;
or a send path that coalesces the 5-byte header with the payload (that
would move large messages onto `pop_from_chunk` for whole-read arrivals
and change the carry volume being gated).
