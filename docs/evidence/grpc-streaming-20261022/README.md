# Streaming fixes and measurements, 2026-10-22

The candidate fixes cancellation, failed-batch rollback and compression handling,
and reduces streaming allocation costs. Its full workspace suite passes.
**Production readiness, complete specification compliance and broad superiority
against tonic/prost remain unproved.**

Candidate: `6cd91911afce24cceee6df3f60a945453e5b04da`.
Comparison baseline: `8fe0f8aeb41356ff427f44542947f36f6d2a2dae`.

## What changed

- Received streams observe local cancellation before returning buffered data or
  successful completion. Completed streams retain their terminal behavior.
- Failed batch construction rolls back only the failed message's bytes and
  releases its reservation. Earlier accepted messages remain intact.
- Streaming encoders reuse buffer capacity and avoid redundant ownership work.
  Compression decoders write into bounded output buffers without an extra
  buffered-reader allocation. Message and decoder-window limits remain active.
- Compression negotiation tests cover missing, identity and gzip declarations
  across request and response shapes. Independent-peer cancellation tests separate
  local cancellation from the peer's terminal status.
- Handler-drop tests wait for actual handler entry before cancellation and
  verify destruction. The tests no longer infer handler entry from a short delay.

For the first Zstd frame, declared content size and checksums are checked.
Decoder history has a separate configurable window limit (8 MiB by default).

No resource or performance threshold was loosened. Cleanup tests now establish
handler entry directly. No new ignores were added; earlier failures are retained.

## Validation

| Check | Source | Recorded result |
|---|---|---|
| Full workspace, all features | `6cd91911` | **3,677 passed**, zero failed, nine existing ignores; 137 targets; 33 min 29 s |
| Lifecycle and handler cleanup | `6cd91911` | Included in the passing workspace: 5,000 seeded lifecycle cycles across the configured transports and handler-drop/reset tests |
| Default libraries, Rust 1.85 | `6cd91911` | Passed in 19.8 s; `pbrs` and `pbrs-grpc`, not every optional feature |
| Warning-strict rustdoc | `6cd91911` | Passed |
| Formatting, campaign contracts and plan lint | `6cd91911` | Passed; 44 campaign-contract tests |
| Strict Clippy | `6cd91911` | **Failed: 115 library diagnostics**; the recorded baseline also has 115 |
| ASAN with leak detection, all-feature library | `7be4f29d` | 639 passed, one existing ignore |
| Finite-limit resource preview | `6cd91911` | Passed; requested 30 s, observed **31.64 s** |
| Actual 24-hour resource campaign | `6cd91911` | **Running at publication**; started 2026-10-22T20:27:47.555424+00:00; completion pending |

The workspace command is `cargo test --locked --workspace --all-features --no-fail-fast`.
Records: `streaming-c3-workspace.*`; other clean checks use `streaming-c3-*`.

ASAN belongs to `7be4f29d`, before the private alias and fixture updates. It is
not an exact-source `6cd91911` run and covers library tests, not all integrations
or platforms. Earlier Miri results retain their own source and selected-primitive
scope; this candidate has no new Miri run.

The earlier `7be4f29d` workspace has 3,670 passes, four failures and nine existing
ignores. Two hostile gzip fixtures omitted the encoding declaration. Cancellation
assertions conflated the local result with an independent peer's completion.
The repaired tests retain wire and cancellation checks; the failed run is archived.

The 87 focused passes belong to their dirty snapshot. Earlier lifecycle/handler
replays retain their source records; counts from separate runs are not added.

## Performance scope

Both matched native campaigns complete **2,400 of 2,400 captures**, with
**720,000 offered, dispatched, completed and successful RPCs each**. Neither has
failed, timed-out, overflowed, unstarted or unfinished calls or status errors.
The candidate takes 563.89 s. The matched all-counter and A/B reports retain
actual RPC accounting; the earlier 600-capture baseline is archived history.

| Campaign | Source | Scope and status |
|---|---|---|
| Matched comparison baseline | `8fe0f8ae` | **2,400 captures passed**, 720,000 successful RPCs |
| Preliminary candidate | `7be4f29d` | 600 captures retained; compiler overlap makes this a diagnostic, not the primary final run |
| Final native counters | `6cd91911` | **2,400 captures passed**: 1/64 KiB, 1/16 in-flight |
| Callgrind instruction pilot | `8fe0f8ae` / `6cd91911` | **144 captures passed**, 5,400 successful RPCs; 25/50-call samples |

The native campaigns run five call shapes, plaintext/TLS, identity/gzip, both
native codecs and a tonic/prost control. Gzip uses level 6. Payloads contain
zero-filled bytes, and streaming calls use four messages per stream direction.
Both revisions cover the same payloads, profiles and loads; selected primary
rows use 1 KiB, one-in-flight cases.
Counters use matched 200/400-call differentials and three repeats. Across the
expanded run there are 480 comparisons per side and metric: client allocation
counts are lower/higher/equal in 393/86/1, and server counts in 334/146/0. Client
requested bytes are lower/higher in 371/109, and server bytes in 274/206.
Each comparison is one workload/codec/repeat differential, not a qualified win.

Each endpoint runs in a separate process against a tonic/prost peer. The pbrs
server uses the full interop handler, including response-plan construction and
status, delay and echo checks. The native prost and tonic handlers use simpler
loops. A pbrs/prost difference includes application work: use native/prost versus
tonic/prost to isolate transport, and retain the same handler for before/after.

Allocations and requested bytes are counted. Requested bytes are cumulative
allocation requests, not retained memory. The final uncompressed prost bidi
server costs 45.8 KiB per RPC versus 32.9 KiB for tonic/prost (+39%). Gzip bidi
server allocations fall from 107.2 to 88.1 with prost, versus 94.0 for tonic/prost.
With pbrs they fall from 122.1 to 107.1 and still exceed tonic/prost.
At 64 KiB, the prost gzip bidi server uses 111.2 allocations versus 99.0 for
tonic/prost (+12%); gzip client-streaming requests use 1,773.8 KiB versus
1,190.6 KiB (+49%). Source comparisons also regress: 64 KiB gzip upload server
requested bytes grow 9.0% with prost (1,667,217 to 1,816,373 bytes/RPC) and 10.6%
with pbrs (1,405,994 to 1,555,148). Gzip bidi bytes grow 3.4% with prost
(3,421,735 to 3,538,220) and 2.6% with pbrs (4,470,487 to 4,586,967).
Allocation-count improvements do not hide these byte regressions.

Native runs collect raw process context switches through `getrusage`; they do
not measure task wakeups or establish a scheduling advantage. Context-switch
ratios are unavailable in six client and 15 server differentials; their partial
counters remain archived. Syscalls are not measured in this phase. Callgrind
applies only to selected instruction cases. Qualified CPU, latency, throughput,
network behavior and scheduling comparisons remain open. The 144-capture pilot
passes with 5,400 successful RPCs across both revisions, using three repeats of
25/50-call samples. Gzip server bidi/pipelined instruction medians are 2.2–2.4%
above tonic and about 0.7% above the old code: allocation savings are not CPU wins.
Prost uncompressed paired medians are 24–28% below tonic. Against the old source,
uncompressed pbrs bidi increases 3.0%, and prost pipelined bidi increases 6.5%.
These use paired repeat ratios, not ratios of independent medians; small-N setup
subtraction and repeat ranges matter. Of 24 native-server instruction comparisons,
17 tie, five regress and two improve under the 2% band. All ranges are retained;
the full 360-capture scope is not run.

The shared Linux x86_64 host has a four-CPU quota and 16 GiB memory limit.
Dedicated x86_64/Arm hosts, realistic field access, broader payload/load coverage,
cold/idle behavior, overload and real-network statistics remain requirements.

## Resource and protocol limits

The actual day uses the passing-preview source, original finite limits and recovery checks. Its durable supervisor retains the terminal result and attempts to publish it separately. A running snapshot is not a completed 24-hour pass.

The preview passes the original finite process limits and recovery checks. It
is not a 24-hour pass. The earlier independent day on `8fe0f8ae`
[failed after 4 hours 56 minutes](../grpc-resource-soak-20261022/README.md).
Fault injection now observes only accepted writes and waits for complete HTTP/2
frames and header blocks. Five deterministic controls fail on the old helper
and pass on the corrected one. The exact day failure was not independently
reproduced; these controls do not establish its unique cause. The failed attempt
remains part of the record.

Compression remains blocked: native gzip/Zstd returns only the first member/frame
and can silently omit protobuf fields in valid concatenated input. Gzip, deflate
and Zstd do not fully validate trailing input. The opaque tonic adapter retains
an independent first-member gzip path; a native-only fix leaves partial data.
Zstd reserved descriptor bit 3 is not rejected. The validated size/checksum
checks cover the first frame, not a complete concatenated compressed message.

The external compression-tail proposal and documentation correction are review
material: **UNCOMPILED/UNTESTED and not applied**. They supply no passing evidence.
A complete fix needs aggregate limits, fragmented/multipart and trailing input,
checksums, adapter behavior, decoder-work bounds and matched performance checks.

Strict lint, platform/release gates and feature commitments remain open, including
external discovery/balancer integration, credential providers, typed extensions,
ALTS qualification, xDS and full telemetry.

## Verify and replay

Run `python3 docs/evidence/grpc-streaming-20261022/check.py` to verify
`raw.tar.gz` and its members against `manifest.json`; integrity is not a pass.
`results.json` scopes outcomes. Command records retain source and dirty-state
hashes, arguments, tool versions, exit codes and logs; build records pin binaries.

Use clean checkouts of each recorded source for replay. Start with the commands
in [Benchmarks](../../benchmarks.md), retain every failed capture, and read the
[benchmark contract](../../benchmark-contract.md) before making a speed claim.
