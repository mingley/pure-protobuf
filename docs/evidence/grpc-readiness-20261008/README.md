# gRPC checks and endpoint measurements, 2026-10-08

Production qualification and the goal of beating tonic remain open. This
record preserves the measurements, failures, and rejected experiments from
this session. It does not rank the implementation.

## Changes

- Explicit shared byte budgets survive channel/server configuration changes.
- Small DATA frames use the configured send-buffer limit. A paused compressed
  reader can no longer make the producer enqueue its entire response stream.
- Load clients send the first bidi request before waiting for response headers.
  A server that waits for that request is now supported by every benchmark arm.
- Fixed-count load runs require every offered RPC to finish successfully.
  Failed captures remain failed when generating a partial comparison ledger.
- A 24-hour report requires an actual day of execution and passing checks.
  The fault-recovery fixture requires a successful RPC on a warmed independent
  connection to the same single-slot server, within the original 300 ms limit.
  Socket lifecycle tests use the same approach; deliberate TLS/mTLS server
  shutdown instead requires the server task to finish within 300 ms.

The benchmark gzip level is 6 for both implementations. Library defaults are
unchanged. These are bounded fixes, not evidence that all release gates pass.

## Retained results

The release measurements use source
`ff65478c1d755d2fcd5e8a8c3cac5b311cb061a9`, before the bidi benchmark fix.
The frozen executable's SHA-256 is
`f231960ce0eee1bfd9bf2bd7d40dfb3d5750de66b7af08d8356bbe13e09d1531`.
The build record, Cargo artifact selection, lock hashes, commands, tools,
endpoint logs, and raw profiler output are in the archive.

| Check | Result | Limit |
|---|---|---|
| Functional endpoint matrix | 2,557 of 2,560 passed | Three original failures retained; 100 ms measurement windows include startup |
| Fixed-count replays of those three cells | 30 of 30 passed, 20 RPCs each | Replays do not replace original failures |
| N=200 endpoint counters | 75 of 75 passed | Shared Linux host, 1 KiB, h2c, identity, one connection/call |
| N=400 endpoint counters | 74 of 75 passed | One bidi capture had four deadline failures |
| Partial per-side ledger | 92 losses, 82 wins, 3 ties, 177 missing, 6 failed | Diagnostic rows, not qualified rankings |
| Resource preview | 33.74 seconds passed | Short preview, not a soak |
| First 24-hour attempt | Failed after 503.99 seconds | Recovery probe timed out during GOAWAY cycle 89 |
| Bidi regression and benchmark suite | 187 tests and strict Clippy passed | Source integrated at `05de89ba`; separate from the measured release |
| Revised lifecycle and resource suites | 15 lifecycle + 5 resource tests passed; strict Clippy passed | 5,000 seeded histories plus TCP/TLS regressions; earlier TLS failures retained |

The functional matrix covers all 16 native/tonic and pbrs/prost endpoint pairs,
five shapes (unary, server stream, upload, bidi, pipelined bidi), four payload
sizes (empty, 1 KiB, 64 KiB, 1 MiB), h2c/TLS, identity/gzip, and 1/16 calls on
one connection. This is functional coverage, not the full performance matrix.

The counter table below uses the median of three matched `(2N-N)/N`
measurements per side. Values above 1 cost more than tonic. Instructions come
from Callgrind; allocated bytes are requested bytes, not retained memory.
Setup is included in each raw capture. Missing or nonpositive differences
stay unmeasured; the failed repeat cannot produce a win.

| Side / codec | Shape | Instructions vs tonic | Allocations vs tonic | Requested bytes vs tonic |
|---|---|---:|---:|---:|
| client / pbrs | bidi | 1.097× | 1.003× | 0.779× |
| client / pbrs | bidi_pipelined | 0.947× | 0.984× | 0.864× |
| client / pbrs | client_stream | 1.118× | 0.538× | 0.856× |
| client / pbrs | server_stream | 1.049× | 0.646× | 0.768× |
| client / pbrs | unary | 0.973× | 0.463× | 0.733× |
| client / prost | bidi | 1.105× | 0.808× | 1.387× |
| client / prost | bidi_pipelined | 0.966× | 0.789× | 1.437× |
| client / prost | client_stream | 1.148× | 0.635× | 1.161× |
| client / prost | server_stream | 1.034× | 0.528× | 0.821× |
| client / prost | unary | 0.980× | 0.409× | 0.777× |
| server / pbrs | bidi | 1.278× | 1.177× | 2.073× |
| server / pbrs | bidi_pipelined | 1.343× | 1.168× | 2.157× |
| server / pbrs | client_stream | 1.502× | 0.807× | 1.267× |
| server / pbrs | server_stream | 1.518× | 0.934× | 1.814× |
| server / pbrs | unary | 1.474× | 0.704× | 1.213× |
| server / prost | bidi | invalid repeat | invalid repeat | invalid repeat |
| server / prost | bidi_pipelined | 1.226× | 0.893× | 1.599× |
| server / prost | client_stream | 1.363× | 0.677× | 1.123× |
| server / prost | server_stream | 1.418× | 0.845× | 1.637× |
| server / prost | unary | 1.366× | 0.624× | 1.175× |

The recovery fixtures were integrated at `2ee3c80b`. These ordinary test
results preceded their commit; `checks/independent-recovery-source.json` records
that source reconstruction. Subsequent API documentation edits change comments
only. The clean-source runner supplies a separate pin for each resource campaign.

## Failures and remaining work

Mixed-load resource fairness still fails intermittently. Reset pre-admission
checks and cooperative yielding did not consistently pass the controls, so
those prototypes were not integrated. An absolute bulk-reader pacing fixture
was also explored and left out. The original thresholds were not relaxed.

The old 24-hour report incorrectly says `completed` because it used requested
duration. That original report is retained unchanged as failed evidence.
Current reporting rejects that label. Historical validator snapshots explain
the earlier schemas; they cannot qualify an old run under the stronger fixture.

The full performance matrix still needs read-all/corpus workloads, saturation,
cold and idle connections, task wakes, context switches, syscalls, actual tonic
TLS-session telemetry, and controlled x86_64/arm64 runs. The observed streaming
instruction and byte losses need implementation work and matched reruns.

Feature qualification is also open: ALTS has no selected record provider;
external discovery plugins, OAuth/per-RPC credentials, typed extensions,
official reconnect/retry-amplification checks, and adoption map wire parity
remain tracked work. A passing resource run cannot close these requirements.

## Verify the archive

Run `python3 docs/evidence/grpc-readiness-20261008/check.py` from any directory.
It checks the archive checksum, exact member inventory, sizes, and every file
hash without extracting files. `manifest.json` indexes the selected raw files.
The frozen executables and build caches are excluded. Paths inside old reports
describe the original workspace; the archive preserves their contents.
