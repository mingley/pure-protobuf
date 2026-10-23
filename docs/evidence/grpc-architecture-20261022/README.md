# gRPC execution, compression and ownership

Runtime source: `783bcd53c723032cd0ccbff5d4550f9a1c92cbb8`.
Benchmark candidate: `804b5c593fc209cbc5714fd3e77710b54711cc63`; matching control:
`2eba72d86b4bb7db133120797e3c4746d86713bc` (runtime `6cd91911`).
Their benchmark code is identical; candidate runtime trees match `783bcd53`.
Controls retain the preceding `90201458`, `a3b1d4cc` and `6cd91911` revisions.
Production qualification and broad superiority over tonic/prost remain open.

## Changes

- Poll ready server handlers and cancellation jobs without scanning idle ones.
- Keep cancellation jobs on their owning runtime, with bounded registration.
- Validate complete compressed messages, including later members, checksums and
  trailing input. Bound decoded bytes, history windows and member/frame count.
- Grow decoded output from observed bytes and decompress opaque tonic uploads once.
- Box the selected generated prost method's future.
- Return responses independently of blocked request upload; retain encoded-tail
  byte charges until enqueue or cleanup.
- Use one atomic cancellation handshake and run waker callbacks outside its lock.
- Remove enum-layout assumptions; publish replacement owners before destructors
  can reenter; release live stream fields at store teardown.
- Give registry caches stable identities and upload tasks reset guards.

## Checks

| Check on `783bcd53` | Result |
|---|---|
| AddressSanitizer with leak detection | 720 passed, 1 existing ignored |
| Default library minimum Rust 1.85 | Passed |
| Zstd library minimum Rust 1.87 | Passed |
| Benchmark/resource controller contracts | 44 passed |
| Day supervisor/publication controls | 22 passed; synthetic reports only |
| Release resource preview | Passed: 10 cycles in 34.27 s, original limits |
| Full workspace | 3,739 passed, 5 failed, 9 ignored; 139 targets |
| Required CI Clippy and strict all-feature rustdoc | Passed |
| Broader gRPC all-target Clippy | Two test-fixture diagnostics; correction pending validation |
| Matched performance captures | Pending |
| Actual 24-hour resource run | Not started |

The sanitizer run includes 686 library, 12 blocked-upload, 18 retry and four
upload-panic/cancellation tests. Independent HTTP/2 peers require exact statuses,
reset delivery, released byte charges and a successful RPC on the same channel.
Every preview drain returned client/server byte charges, byte tokens and active
streaming calls to zero; one shared cancellation driver remained. These checks
do not cover every feature, platform or production workload.

The benchmark uses a common 400 KiB server send buffer and 16-message client
request queues. This matches the locked tonic/Hyper server default without
changing shipping native defaults. Native channelz/accounting remains included;
prost routes keep handler work equal. Older captures used different settings
and cannot serve as a matched runtime-only baseline for these captures.

## Current workspace failures

Four serving tests and one Tower deadline test fail at `783bcd53`. Isolated
quiet replays pass 14 of 15 repetitions; one cancellation fixture still fails.
Deterministic fixture repairs now retain the same deadlines and statuses while
using explicit admission, handler-poll and destruction witnesses. The repaired
Tower case and two serving cases pass development checks. The first controlled
connection-age replay exposed a fixture that froze time before its grace timer
could expire; it now observes retirement before advancing the 1 ms grace.
That replay, cancellation and negative controls remain pending. Production code is unchanged by these fixture repairs. The
original failure remains authoritative; no new full-workspace pass is claimed.

The environment transition interrupted the outer recorder. A detached observer
retained Cargo’s actual kernel exit status (101), its process identity, all 139
complete result summaries, clean-source checks and log hashes. The missing
original recorder result is not reconstructed as a success.

## Controls and earlier attempts

The archive retains failures and intermediate harness corrections:

- At `90201458`, the workspace had 3,717 passes, three failures and nine ignores.
  Two reset assertions expected the wrong code; the TLS deadline fixture raced
  handler admission. The corresponding corrections are included in this source.
- The `a3b1d4cc` workspace build was superseded during compilation, before any
  tests ran. Its completed sanitizer and minimum-version checks remain separate.
- Old two-atomic cancellation and reentrant-waker controls fail; replacements
  pass. The Loom model covers the handshake, not the entire transport.
- Old registry default/identity controls fail; corrected controls pass.
- Original blocked-upload and background-panic controls reproduce hangs without
  RPC deadlines. Positive controls retain ordinary cancellation behavior.
- The earlier `6cd91911` day failed after 32 min 2.73 s. Its cause was not uniquely
  reproduced by 2,008 scenario replays; the failed attempt remains published.

The independent TCP scheduling control at `90201458` reduces unrelated idle
handler polls from 80 / 5,120 / 20,480 to zero at 1 / 64 / 256 idle streams.
It proves that mechanism, not lower latency. Its generated prost dispatch
control measures unary/chat futures at 1,808 / 88 bytes versus 2,976 / 2,976
for direct dispatch. Output lookahead probes measure allocation requests;
they do not measure retained memory or RPC speed.

## Evidence

`manifest.json` indexes the source changes, commands, raw logs, failures,
controller sources and hashes in `raw.tar.gz`. Executables are represented by
hashes. Verify archive integrity with `python3 check.py`; integrity is separate
from qualification. Comparative results will be linked from
[benchmarks](../../benchmarks.md), with scope and remaining losses.
