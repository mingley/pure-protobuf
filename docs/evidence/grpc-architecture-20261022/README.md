# gRPC execution, compression and ownership

Runtime and benchmark source: `9d6fb9c416fc32732450fd78d32ee16ff8b802de`.
Matched benchmark control: `2eba72d86b4bb7db133120797e3c4746d86713bc`
(runtime `6cd91911`). The control and candidate use identical benchmark code.
Fixture checkpoint: `deb0d1d6b9855188a23128c0dca54c03dc2c8bfc`. It changes only
`pbrs-grpc/tests/serving.rs`; the complete diff and its hash are retained. All
other tracked inputs are byte-identical. Results keep their actual source pins.

Production readiness, full specification coverage and broad superiority over
tonic/prost remain unproved.

## Changes

- Poll ready handlers and cancellation jobs without scanning idle ones.
- Own cancellation through handler teardown, including connection task abortion
  and forced connection-age cleanup. Remove abandoned waiter registrations in
  constant time; storage follows peak live registrations rather than churn.
- Validate complete compressed inputs, including later members, checksums and
  trailing data. Bound decoded bytes, history windows and member/frame count.
- Grow decoded output from observed bytes and decompress opaque tonic uploads once.
- Box the selected generated prost method's future.
- Return responses independently of blocked request uploads; retain encoded-tail
  byte charges until enqueue or cleanup.
- Use one atomic cancellation handshake and invoke waker callbacks outside locks.
- Publish replacement owners before reentrant destructors, release live stream
  fields at teardown, and give upload tasks reset guards.
- Test deadlines after explicit admission with controlled time, unchanged timeout
  values, before-expiry checks and actual handler destruction witnesses.

## Validation

| Check | Source | Result |
|---|---|---|
| Full all-feature workspace, 139 targets | `deb0d1d6` | **3,755 passed, zero failed, nine ignored**; 13 min 25 s |
| AddressSanitizer and leak detection | `9d6fb9c4` | **731 passed**, one existing ignore |
| Serving fixture suite, strict Clippy and formatting | Corrected file committed at `deb0d1d6` | 1,100 passed; checks passed |
| Copied-fixture detecting controls | `deb0d1d6` | 15 positive cases passed; 28 broken cases failed at expected assertions |
| Ten pre-workspace checks | `9d6fb9c4` | Passed: strict lint, rustdoc, formatting, examples, 193 benchmark tests and Rust 1.85/1.87 library checks |
| Descriptor parity, protoc 35.1 release compiler | `deb0d1d6` | Separately passed; not the official C++ conformance runner |
| Resource preview | Runtime `9d6fb9c4`, qualified by fixture checkpoint | Passed: nine drains in 30.54 s; 18.5 MiB sampled peak RSS |
| Corrected benchmark captures | Runtime `9d6fb9c4` against matched control | Passed: 3,000 captures and 845,790 successful RPCs |
| Actual 24-hour resource run | Runtime `9d6fb9c4` | Started 2026-10-23 at 02:52 UTC; result pending |

The full run starts and ends at a clean fixture checkpoint. The sanitizer and
release artifacts retain the earlier runtime pin; they are not relabeled as
new builds. The ignored compiler and resource tests are not silently counted as
passes. Edition 2024 source compiler oracles and the official C++ conformance
job are separate gates. Offline controller checks remain synthetic.

Every preview drain released client/server byte charges, byte tokens and active
streaming calls to zero. Drains had seven file descriptors and one shared
cancellation driver. Sampled RSS peaked at 18.5 MiB; the separately observed
process RSS high-water mark was 20.0 MiB. The original 1 GiB address-space and
128-descriptor limits were retained. These nine cycles do not qualify a day.

The runtime's eleven new teardown/waiter controls fail against the former
mechanisms and pass against the replacement. Eight use independent registry h2
TCP peers across all four RPC shapes; three cover waiter churn and reentrancy.
They prove the detecting mechanisms, not complete production qualification.

## Performance

All six phases passed their functional and accounting checks. Every offered RPC
was dispatched, completed and successful; failed, timed-out, unstarted and
unfinished counts were zero.

| Campaign | Captures | Successful RPCs |
|---|---:|---:|
| Selected matched source comparison | 400 | 120,000 |
| Instruction pilot | 4 | 60 |
| Oversized pilot | 4 | 150 |
| Oversized matched source comparison | 120 | 4,500 |
| Expanded current allocation matrix | 2,400 | 720,000 |
| Focused instruction comparison | 72 | 1,080 |
| **Total** | **3,000** | **845,790** |

See [benchmarks](../../benchmarks.md) for readable tables. Equal prost-handler
64 KiB gzip upload server requests fell 14.5%, but remain 27.4% above tonic.
The larger remaining byte loss is 1 MiB gzip bidi: native is 71.0% above tonic
on the client and 75.5% on the server, despite saving about 4 MiB per endpoint
against the preceding source. Small unary client requested bytes rose 2.5%,
and gzip server-stream allocation count rose 0.9%; these regressions remain.

The focused instruction diagnostic uses 64 KiB gzip, plaintext, one connection
and one in-flight call, three seeded repeats and 10/20-call subtraction. Prost
upload server instructions fall 19.5% against the old source and 4.8% against
fresh tonic controls. Prost bidi falls 3.8% against the old source; its 1.6%
reduction against tonic remains inside the 2% tie band. Both endpoints are
instrumented, but the client is tonic: native client instruction coverage is
still open. The pilot and expansion use 304.19 profiler seconds in total.

The corrected harness gives both servers a 400 KiB send buffer and both clients
16-message request queues. Shipping defaults are unchanged. Native channelz and
accounting remain enabled. Prost routes keep application work equal; pbrs interop
routes include extra handler work. These compare APIs, not bare transport kernels.

The expanded candidate matrix and selected matched source comparison have
different coverage. Allocation requests do not measure retained memory.
Allocation-instrumented Callgrind counts do not establish shipping latency,
throughput or uninstrumented CPU superiority. Missing counters remain unavailable.

## Retained failures

The original `783bcd53` workspace failed: 3,739 passed, five failed and nine
ignored across 139 targets. A detached observer retained the actual Cargo exit
101, complete summaries, source checks and raw log hashes after the outer
recorder was interrupted. The missing recorder result is not treated as success.

The `9d6fb9c4` workspace failed: 3,753 passed, two failed and nine ignored across
139 targets. One deadline expired before handler admission. The other returned
a redacted Internal status; its original message was not recovered. Both passed
quiet replays. The strict five-second header assertion depends on time not
advancing: after admission delay, the client may correctly send a reduced
remaining timeout. This dependency does not establish that failure's unique cause.

The archive also retains intentionally superseded builds, test lint failures,
failed fixture development attempts and the final corrected checks. Offline
controller tests are synthetic; they are not resource runs or performance captures.

Earlier actual resource-day attempts failed. The `6cd91911` attempt failed after
32 min 2.73 s; 2,008 scenario replays did not uniquely reproduce its cause. A
short resource preview and an active day run do not constitute a completed day.

## Remaining work

Measure read-all nested-message decoding, sparse access, both endpoint CPU costs,
latency, throughput and retained memory on controlled hosts. Compare independently
scheduled admitted RPCs against inline execution for mixed expensive/cheap calls
on one connection, preserving ownership, cancellation and finite admission.
Source inspection models roughly 12 MiB of excess allocation requests per
endpoint in 1 MiB gzip bidi: discarded encoding scratch, repeated decoded-buffer
growth and oversized compressed-output reservations. This is a hypothesis, not
allocation-site attribution. The next experiment reclaims backing storage only
when uniquely owned and retains scratch only under finite aggregate leases.
Lazy pbrs values may retain decoded backing; that must remain a cache miss.

Complete remaining features, platform and network coverage, specification gates
and a full recorded resource day before claiming production qualification.

## Evidence

`results.json` identifies final records and their scopes. `manifest.json` indexes
commands, raw logs, failures, controls, source changes and hashes in `raw.tar.gz`.
Executables are represented by hashes. The archive includes a frozen launch and
progress snapshot for the active resource day; live logs belong to its separate
terminal capsule. Success, failure or incomplete termination is published with
a normal update to `main`. Run `python3 check.py` to verify archive
integrity; integrity is separate from qualification.
