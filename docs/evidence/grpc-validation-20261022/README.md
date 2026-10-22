# Production and performance validation, 2026-10-22

Production readiness and broad superiority over tonic/prost are **not established**.
This investigation found correctness defects and measurement defects. The repairs
and retained failures below replace assumptions with scoped evidence. They do not
close QG-06 or SB-27.

The delivered code revision is `8fe0f8aeb41356ff427f44542947f36f6d2a2dae`.
It includes the server URI-cache repair after `6520fb7e`, which fixed benchmark
accounting. Earlier results belong to their exact recorded revisions, not
automatically to the delivered code.

## Confirmed defects and repairs

| Defect | Evidence | Repair |
|---|---|---|
| Optional tonic builds used obsolete channel fields; two tests used the old cancellation API | Initial all-feature build fails with 16 library errors | Access current overlays and cancellation handles; repair reactor-dependent test setup |
| Linux mutex guard was `Sync` for a non-`Sync` protected value | Original compile-time control fails; Miri reports a data race using safe callers | Tie guard auto-traits to the protected value; test contention, handoff and unwind |
| Linux futex code compiled on other platforms | Focused Apple target control fails to compile | Use the standard mutex outside Linux; focused cross-compile passes |
| Concurrent cancellation waiters replaced one another | Two-waiter control loses the first wake | Retain the primary inline waker and register secondary waiters; broadcast cancel and sender closure |
| Replacing a secondary waker deadlocked if its destructor cancelled the RPC | Safe `Wake` control times out | Clone and drop wakers outside the registry lock; native and Miri controls pass |
| Stream cleanup and driver handoff lost required work/wakes | Backoff hangs and nine admission failures retained | Invalidate idle shortcuts and retain/register the current driver waker |
| Lazy extension publication violated reference aliasing | Miri rejects the original shared-reference mutation | Publish through `OnceLock`; preserve user values without hidden clones |
| Owned HTTP strings lost allocation provenance | Miri rejects deallocation of an overallocated empty string | Obtain the pointer from the owning byte vector |
| Bitwise copying a cached request did not retain an owned URI | Actual full-library ASAN use-after-free; isolated Miri control also fails | Clone the request; retain URI/authority owners in pointer caches |
| Pointer-only HPACK matches could replay changed text; request pseudo owners leaked | Same-address changed-text regression and ownership checks | Match live text, retain owners and release recycled pseudo fields |
| Load balancing discarded a pending acquisition and picked again | Existing ORCA and health-balancer failures | Keep the same acquisition future alive through completion |
| Queued stock headers depended on the encoder's thread-local pool | Cross-worker control decodes zero fields instead of four; stock-response classifier control fails | Frames own fields or reconstruct a validated stock kind; classify every required response field |
| Handler request construction consumed the path needed by response interceptors | Existing all-shape response-hook tests fail without an observer | Preserve the path for both handler and outbound response |
| Server path caching could confuse RPC methods after buffer reuse | Exact shipping-code control selects stale text at a recycled address; lifecycle reports an unexpected unary message count | Compare live URI text and retain owned parsed values; remove the non-owning pointer shortcut |
| Benchmark count and concurrency did not match requested workloads | Old code completes 201 for a 200-call cap and reaches four in-flight calls for a cap of one | Reserve counts atomically and apply the closed-loop worker limit |

The changes also repair standalone lockfile metadata, update the dispatch test to
its existing selected-method contract, remove duplicate private API documentation,
fix the client interceptor example, and make scheduled safety jobs exercise the
embedded backend. Test deadlines, resource limits and performance thresholds were
not loosened.

## Results and limits

| Check | Revision | Result | Scope |
|---|---|---|---|
| ASAN with leak detection, all features | `8fe0f8ae` | 619 passed, one existing ignore | gRPC library tests on Linux x86_64 |
| Selected Miri groups | `8fe0f8ae` | 31 passed | Selected shipping lock, store, header, frame, cancellation, extension and ownership tests |
| Reentrant-waker repair under Miri | `8c1703af` cancellation module | Six passed | Five shipping cancellation tests plus the external control; tested before commit with the same module content |
| Native load-generator suite | `6520fb7e` load module | 22 passed | Actual standalone benchmark tests; two new controls fail on the old code |
| Rust 1.85 check | `8fe0f8ae` | Passed | Default-feature `pbrs` and `pbrs-grpc` libraries; not all optional features |
| Formatting and campaign contracts | `8fe0f8ae` | Passed; 46 Python tests | Workspace formatting and capture/provenance/failure handling |
| Strict Clippy, stable 1.99.1 | `8fe0f8ae` | Failed: 115 library diagnostics | Includes 56 unsafe blocks without required safety comments; further targets are not certified |
| All-feature gRPC suite | `8fe0f8ae` | 2,909 passed, three failed, three existing ignores | 62 library/binary/integration targets; lifecycle and serving failures remain |
| Full all-feature workspace | `8c1703af` | 3,639 passed, six failed, nine existing ignores | All 136 test/doc-test targets completed before the URI-cache repair |
| Warning-strict rustdoc and public docs | `6520fb7e` plus documentation edits | Passed; 25 documentation tests | Workspace all-feature rustdoc and links/anchors/snippets/contracts |
| Unpacked-crate consumer checks | `d989a116` | Two passed | Actual core/native/tonic consumer checks with current Cargo |
| Resource bounds, health LB and ORCA | `d989a116` | 40 passed | Existing integration checks after the header repair |

The host is shared Linux x86_64 with a four-CPU cgroup quota and a 16 GiB memory
limit. Toolchains include Rust 1.85.0, 1.88.0, stable 1.99.1 and
nightly-2025-06-26. `source-host.json`, command records and the release build
records retain source, tool versions, flags and binary hashes.

The first broad workspace attempt mixed source edits and consumer build setup
problems. It is exploratory evidence. The clean `f6cec7a4` all-feature run records
3,629 passed tests and 16 failures across five targets. Two documentation/plugin
targets lacked rustfmt in the stable toolchain; installing it repairs the
execution environment. Caller-rustls needed one missing local dependency entry in
its lockfile. Other failures included client-stream half-close scenarios, missing
response paths, duplicate API documentation and one deadline test. Those failures
remain in the archive even where a later replay passes.

The final gRPC failures are:

- `generator_1000_seeded_cycles`: cycle 341, seed `0x1e231424593602b7`,
  server streaming over `from_io`, cancellation at `TrailersReceived`; the call
  reports OK instead of CANCELLED. The URI-cache repair resolves a different
  incorrect-message-count failure, but does not close this cancellation case.
- `from_io_hanging_handler_drops_on_call_drop_and_reset` and
  `unix_hanging_handler_drops_on_call_drop_and_reset`: the handler does not
  start within the original assertion window. No deadline was relaxed.

Miri selected tests exercise particular primitives. They do not constitute Miri
coverage of the full gRPC library or every unsafe path. ASAN library checks do not
cover every integration, fuzz, platform or deployment case. The Apple check only
compiles the selected mutex module; it is not a full macOS build or runtime test.

## Performance evidence

`counter-native-current/` preserves all 600 attempted captures from the
`f6cec7a4` release build. Its manifest declares 1 KiB, one connection, one
in-flight call, all five shapes, all four TLS/compression profiles, three repeats
and matched 200/400-call counts. **The declared concurrency was false:** the old
closed-loop generator used four workers. Two captures completed 401 calls when
400 were requested. The campaign exits unsuccessfully. Do not use this campaign
or its loss ledger as a valid comparison for the declared load.

The older `abeb7bcf` native matrix also used the same four-worker generator. Its
7,200 raw captures remain archived (7,182 original runner passes and 18 failures),
but they do not establish results at the declared load levels. The evidence index
and historical record now state that limitation.

The count/concurrency regressions fail against the old module and pass against
the repaired module. Captures after the repair are kept separately. A native
counter campaign measures allocations, requested bytes and process context
switches. Instruction and syscall phases, task wakeups, read-all application
corpora, cold/idle behavior, saturation and dedicated-host statistics remain
separate requirements. Context-switch differences without a measured scheduling
noise band do not establish a production scheduling advantage.

The corrected `8fe0f8ae` campaign in `counter-route-final/` completes all 600
captures with the required 200/400 successful RPC counts. The source, release
binary and controller drift checks pass. These are 1 KiB, one-connection,
one-in-flight, five-shape, four-profile diagnostics with three repeats.

| Per-side metric | Wins | Losses | Ties |
|---|---:|---:|---:|
| Client allocations | 96 | 24 | 0 |
| Server allocations | 69 | 50 | 1 |
| Client requested bytes | 108 | 12 | 0 |
| Server requested bytes | 84 | 36 | 0 |

Each count is one workload/codec/repeat N/2N differential, not a vote for an
overall winner. The ledgers retain every row. For example, h2c gzip bidi with
prost costs about 107.1 server allocations per RPC versus 94.025 for tonic/prost
(three-repeat medians, about 14% more). This phase does not measure instructions,
syscalls or task wakeups and does not qualify shared-host latency or throughput.

The final `8fe0f8ae` release resource preview passes the original finite limits,
TLS/gzip profiles and recovery checks in 30.36 seconds. An earlier
`f6cec7a4` preview also passed in about 33.6 seconds.

The actual 86,400-second diagnostic started at **2026-10-22 04:35 UTC** on the
frozen `8fe0f8ae` checkout. At the retained snapshot it is running, not completed.
`day-running-snapshot.json` records the live test PID, process identity, binary
hash and elapsed time. The day uses the original finite limits and validators.
No elapsed day is inferred from a preview or a configured duration.

The resource day is an independent diagnostic: the three native failures still
block production readiness. Its supervisor is configured to archive the terminal
outcome and push dated resource evidence directly to `main`. A missing terminal
record is not a pass. This capsule excludes the changing day files; the completed
resource archive is published separately. `results.json` summarizes this snapshot.

## What still blocks the requested claim

- Resolve remaining exact-source workspace and strict-lint failures; review the
  unsafe invariants and qualify all supported platforms and release gates.
- Pass the original lifecycle, recovery and 24-hour resource requirements on the
  final candidate. Preserve every failed attempt.
- Finish the per-side comparison matrix with correct counts and concurrency,
  including payloads, TLS/compression, real read-all corpora and lifecycle cases.
  Record losses in instructions, allocations, requested bytes, syscalls and
  scheduling; missing metrics cannot count as wins.
- Meet the existing paired statistics, latency, throughput and headroom criteria
  on dedicated x86_64 and Arm machines with real-network coverage.
- Complete the remaining feature commitments. The ready queue still includes
  OAuth/per-RPC credential providers, external discovery/balancer integration,
  typed extension generation and ALTS platform qualification. The broader plan
  retains xDS and telemetry work. Task-card completion is not product qualification.

## Check and replay

Run `python3 docs/evidence/grpc-validation-20261022/check.py` to verify the archive
and every member against `manifest.json`. Integrity verification does not turn a
failed or incomplete run into a qualification pass.

Earlier filenames containing `final` describe that stage of the investigation;
their recorded source hashes remain authoritative. `delivered-source.json`
identifies the delivered code.

The archive retains command JSON, stdout/stderr, source snapshots, the delivered
patch, source/host records, control harnesses, resource samples, benchmark captures
and ledgers. Large executables are omitted; their sizes and SHA-256 hashes are in
`omitted-executables.json`. Source snapshots and Git revisions identify the code.
Some early controls used an uncommitted test patch; their command records retain
its diff hash, while later checked-in regressions preserve the behavior tested.
Setup retries include missing tool components and mistyped target/source pins;
they do not count as final code failures. Historical scratch harnesses contain workspace paths; relocate those paths when
replaying them. The tool wrapper documents the local Cargo cache and toolchains.

Use a clean checkout of the recorded revision for each replay. The principal
commands are:

```sh
cargo test --locked --workspace --all-features --no-fail-fast
cargo clippy --locked -p pbrs-grpc --all-targets --all-features -- -D warnings
cargo +1.85.0 check --locked -p pbrs -p pbrs-grpc --lib
cargo test --locked --manifest-path rpc-bench/Cargo.toml load::tests::
python3 scripts/miri-grpc-kernels.py
```

For Miri, select nightly-2025-06-26 and set
`MIRIFLAGS='-Zmiri-disable-isolation -Zmiri-strict-provenance -Zmiri-preemption-rate=0.1'`.
The final ASAN command and `detect_leaks=1` environment are in
`asan-route-followup.json`. Frozen benchmark/resource controllers require their
recorded clean source pin and a new output directory. Their reports explicitly
keep `qualified: false` on this shared host.
