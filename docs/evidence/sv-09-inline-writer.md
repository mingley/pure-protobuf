# SV-09 response-writer box experiment

The inline response-writer experiment is **rejected**. It saves one allocation
and 3,096 heap bytes per native prost stream, but neither instructions nor
allocation count reaches the unchanged 2% targeted-win minimum. Shipping keeps
the original boxed writer; four independent cancellation/drop-order fixtures
are retained.

## Pins and hypothesis

- Baseline runtime: `cf3eee22c6b06324e299f81b76915651471d8025`.
- Original-helper fixture: `cbfac8de`.
- Experimental source: `1ea1d3a0eed240639d34078e9e542e3b97a659f5`.
- Candidate binary SHA256: `a072cfc579888e6d811c2dc90e96ad3125167f6887ebaa22e5eb2105b5ea8fdb`.
- Baseline binary SHA256: `73fa59c733f0637b22ce8d58f035aa45cb13f0b7efc4c8e9bb4cdddcef54e0b9`.
- Runtime restored in `739fe194`; no closed-producer yield change is included.

Baseline disassembly proves the server response-writer allocation and memcpy
are 3,096 bytes (`0xc18`). The bounded hypothesis replaces that box with safe
async-frame pinning. An owned initial state declares cancellation before the
writer and is captured whole through `into_parts()`. After first poll, a
`poll_fn` closure owns the cancellation guard and borrows the pinned writer.
Ready completion drops the guard before writer cleanup, and abandonment keeps
the same cancellation-before-writer ordering. No unsafe code, dependency,
wire/default/security change or threshold adjustment is introduced.

The private helper's manually polled Ready retention differs: the original
boxed helper can retain its completed writer until the helper is dropped; the
async helper drops that completed writer during its Ready poll. Every production
caller awaits the helper and immediately drops its completed state. Validation
checks Ready through that production `.await` usage.

## Correctness and constructor sizes

The four behavioral tests pass first on the original helper, then on the exact
candidate. They cover unpolled drop, Pending abandonment, Ready through await
and writer-poll panic. The writer observes cancellation false while polling and
true before its Drop. All 84 selected integration tests also pass, including
the 5,000 seeded lifecycle scenarios, hostile peer, RPC, burst/error/refill,
direct wire-backed echo, cancellation, size-limit and byte-budget fixtures.
There are no failures or ignored cases.

The candidate uses the same pinned env.sh, jobs=1, standalone locked devloop
manifest and release profile as the baseline. RUSTFLAGS, target CPU and release
overrides remain unset; thin LTO and one codegen unit are unchanged. Release
build completed in 4m20s; the isolated target remains 1.6 GB. Constructor
disassembly retains these larger allocations/copies unchanged:

| Constructor | Baseline | Candidate |
| --- | ---: | ---: |
| Native prost DynService dispatch | 11,536 bytes | 11,536 bytes |
| Native prost client server_streaming body | 10,816 bytes | 10,816 bytes |

These are the concrete erased future bodies, distinct from the small public
Call wrapper. Removing the secondary writer box leaves both dominant fixed
future-copy sites intact.

## Paired primary result

The original collector ran N200/400, repeats3, on both frozen source-pinned
binaries. The quiet lease ran from 2026-10-02 20:53:02 to 20:53:16 UTC; process
states are retained, including stopped/zombie distinctions. All 12 Callgrind
children exited zero. No wall/CPU qualification claim is made.

| Prost server stream metric | Baseline | Candidate | Change |
| --- | ---: | ---: | ---: |
| Instructions/RPC | 158830.095 | 158254.430 | -0.3624% |
| Allocations/RPC | 57.025 | 56.025 | -1.7536% |
| Heap bytes/RPC | 62632.640 | 59536.640 | -4.9431% |
| Syscalls/RPC | 31.395 | 30.685 | -2.2615% |
| Futex-family calls/RPC | 15.010 | 15.215 | +1.3658% |

The unchanged comparator exits zero: no metric exceeds its existing 2% RPC
regression limit. That comparator result does not enforce the targeted-win
minimum. Instruction and allocation-count gains are both below 2%, so the
experiment stops here. Compressed and full20 controls are `not_run` after this
rejection. A heap-byte reduction alone does not satisfy the current win rule.
SV09's matched tonic instruction/byte floor remains unaddressed.

## Retained evidence

`sv-09/inline-writer-evidence.tar.gz` retains the recorded hypothesis and exact
source design, original/candidate test logs, build log, source/binary/tool pins,
original reports and complete raw tool stderr, final-repeat N/2N callgraphs,
after self/inclusive/caller attribution and before/after constructor disassembly.
Its file hashes are in `sv-09/inline-writer-manifest.json`.
`sv-09/rejected-inline-writer.patch` reconstructs the experiment from the
original-helper fixture source. Baseline tools, lock and build flags are shared
with the source-pinned capture described in `sv-09.md`; binaries are not bundled.

Reproduce with the transparent wrapper and separate PBRS_REPLAY_RAW destinations,
PBRS_DEVLOOP_COMMIT source labels, and frozen binaries:

```sh
devloop run --cells rpc.prost.server_stream --iters 200 --repeats 3 --out REPORT.json
devloop compare --rpc --baseline BEFORE.json --current AFTER.json
```

The full targeted-win rule must still be checked independently from comparator
exit status. Allocation-stack tracing and sampled flamegraphs remain `not_run`
for the reasons recorded in the current-source attribution evidence. Existing
Callgrind profiles provide instruction attribution without inventing unavailable
allocation-stack evidence.

## Restored-source fixture lint proof

Fixture-only source `b6c2c69f8fd0498119591d4aa600df62b758745d` passes strict
`cargo clippy -p pbrs-grpc --lib --tests --features prost,copy-counts -- -D warnings`,
then all four cancellation unit tests and all five streaming integration tests.
The original strict run fails on the older Waiting fixture's two standard Mutex
references. Narrow, reasoned type-alias expectations retain its single-use sender
take and the new synchronous poll/drop event recording. Every lock guard ends
within its statement before any await; no runtime policy or helper changes.

`sv-09/restored-fixture-lint-proof.tar.gz` retains the original red lint, final
green lint and nine test results, exact final fixture sources and completed-debug
cache cleanup log. Hashes, source and build environment are recorded in
`sv-09/restored-fixture-lint-manifest.json`. After retaining proof, the isolated
target's completed debug artifacts were pruned; the retained release cache is
942 MB. This fixture proof does not qualify either rejected runtime experiment.
