# SV-09 borrowed request-timeout experiment

The borrowed-timeout experiment is **rejected** under the unchanged targeted-win
rule. It reduces native prost stream instructions by 1.6877% and heap bytes by
2,304 per RPC, while allocation count stays unchanged. The required targeted
instruction or allocation-count improvement is at least 2%. Shipping retains
the original owned timeout call sites; eight independent lifetime/deadline/reset
fixtures remain useful. No full-control or adoption sweep follows this rejection.

## Source, tools and bounded hypothesis

- Baseline runtime: `cf3eee22c6b06324e299f81b76915651471d8025`.
- Original-runtime fixture source: `e75a1b593f831c5c4b2fff4ea8f62bbe795212b9`.
- Experimental source: `6ffafa94b6c66144d4afd7b7156859f249c90141`.
- Runtime restored: `7db67c364020c21f3fa476487bf3d2bf5eb9943d`.
- Baseline binary SHA256: `73fa59c733f0637b22ce8d58f035aa45cb13f0b7efc4c8e9bb4cdddcef54e0b9`.
- Candidate binary SHA256: `8c5e022e57583c1cbeac425690abec7a9de654db367625f7535e093340863fab`.

Both binaries use the original locked standalone `bench/devloop` inputs,
toolchain `env.sh`, jobs=1, thin LTO and one release codegen unit. Rust/Cargo are
1.99.0; Valgrind is 3.24.0. RUSTFLAGS, target-CPU and release overrides remain
unset. Manifest, lock, build script, collector, tool, binary and source hashes
are retained. No dependency, unsafe code, protocol, default or threshold change
is introduced. Prior closed-channel, inline-writer and Option-slot experiments
are not included in this candidate.

Only two `server/rpc.rs` request-phase call sites change. Each existing async
phase is named, pinned once in its owning scope, and borrowed as `Pin<&mut F>`
through the unchanged `wrap_timeout`. The scope ends before `notify_deadline`
and `Prepared`, preserving completion, cancellation and receiver-drop timing.
Both request bodies retain exactly the same normalized tokens. `run_handler`,
the boxed response writer, dispatch boxing, producer yielding and client code
remain unchanged. The hypothesis is that replacing repeated owned timeout
future storage with a borrowed future shrinks actual service construction.

## Structural prerequisite and attribution

Release disassembly of the actual native `ProstNativeEchod` dispatch, rather
than a toy future, shows the following allocation and constructor-copy sizes:

| Actual future storage | Baseline | Candidate |
| --- | ---: | ---: |
| Native prost DynService dispatch | 11,536 B | 9,232 B |
| Native prost client server-streaming body | 10,816 B | 10,816 B |
| Boxed response writer | 3,096 B | 3,096 B |

The dispatch still performs one allocation and a whole-future memcpy. The
2,304-byte reduction passes the structural prerequisite, but does not by itself
qualify a runtime change. Its measured heap-byte reduction later matches that
size exactly, without an allocation-count change.

The final N400 before/after whole-process callgraphs retain memcpy self cost
17,261,674 → 16,105,636 instructions. Dispatch-attributed memcpy falls
4,674,267 → 3,748,747 instructions; client-constructor memcpy remains
4,438,987 → 4,441,067. These diagnostic totals include startup and warmup;
they are not differential per-RPC shares. Complete profiles plus self, caller
and callee annotations preserve the underlying attribution.

## Correctness

Eight paired fixtures pass first on the original owned helper shape. They
cover unpolled and Pending abandonment, Ready completion, poll panic and
elapsed deadline drop order, plus the actual handler's Ready-over-reset bias,
one cooperative cancellation poll and retained final cooperative result.
The phases are `!Unpin`; no unsafe pin construction is used. These fixtures and
the existing four writer-cancellation fixtures pass on the exact candidate.
Strict lib/tests Clippy with `prost,copy-counts` passes on fixture and candidate
sources.

The candidate also passes all 110 selected all-feature checks: 12 cancellation
and timeout units plus 98 transport tests across RPC, trailers/deadlines,
burst/error/refill/direct-wire echo, timeout-header regression, gRPC-Web, bidi
completion, hostile peers, message-size limits, byte budgets, compression and
lifecycle. The lifecycle suite includes 5,000 seeded transport scenarios.
There are no failures or ignored tests. Completed dedicated test executables
are hashed before reclamation. At packaging, the completed owned target is
2,079,985,952 bytes, below 2 GiB, with no active command or executable using it.
Packaging deletes no cache and makes no historical peak-memory claim.

The restored `rpc.rs` is byte-identical to the pinned baseline. Original-runtime
fixture checks and the candidate checks are retained separately; no new compiler
replay after restoration is claimed.

## Original paired primary screen

The coordinator grants a quiet lease with other builds and captures held. The
original collector runs `rpc.prost.server_stream` at N200/400, three repeats, baseline
then candidate, from 2026-10-02 22:32:56.040 to 22:33:10.137 UTC. All 18 tool
children exit zero: 12 Callgrind children and six `strace -c -f` children.
Every raw stdout/stderr, argv, exit status and all 12 complete callgraphs are
preserved and hashed. Independent parsing reproduces all reported medians.

| Prost server-stream metric | Baseline | Candidate | Change |
| --- | ---: | ---: | ---: |
| Instructions/RPC | 158837.455 | 156156.800 | -1.6877% |
| Allocations/RPC | 57.025 | 57.025 | 0% |
| Heap bytes/RPC | 62632.640 | 60328.640 | -3.6786% |
| Syscalls/RPC | 31.835 | 31.385 | -1.4135% |
| Futex-family calls/RPC | 15.450 | 15.500 | +0.3236% |

The unchanged comparator exits zero because every screen metric remains within
the existing 2% RPC regression limit. It does not enforce the targeted-win
minimum. The instruction gain is below 2% and allocation count is unchanged,
so byte savings alone do not qualify this experiment. Full20, compressed,
adoption and further tonic control measurements are `not_run` after rejection.
SV-09's matched tonic instruction/heap-byte floor remains unaddressed.

This is the unchanged legacy loopback workload: one streaming call settles the
connection before the allocation window, then measured calls run sequentially.
The multi-thread Tokio runtime leaves worker count at its default. The legacy
row does not consume the generic `--warmup` setting. Differential Callgrind
instructions include client, server and process helpers. Strace reports the
whole process, including startup/warmup, divided by N; it is not differential.
These results do not establish isolated production-server or wall-time gains.

The manually supplied lease annotation incorrectly says approximately 22:34.
The original metadata remains intact. An additive clock-correction record
retains that original text, the actual capture timestamps and the coordinator's
grant. No measurement timestamp or raw tool evidence is rewritten.

## Retained proof and reproduction

`sv-09/borrowed-timeout-evidence.tar.gz` retains the hypothesis, exact experimental
patch, source/tool/build pins, before/after assembly and callgraph annotations,
original/candidate correctness and strict-lint logs, 110 all-feature gate logs,
all raw tool captures and full profiles, reports, comparator and independent
median/threshold verification, wrapper smoke checks and additive clock correction.
The two compressed raw stage archives each contain all nine tool children and
all six full profiles. Individual raw file hashes and archive hashes are in
`sv-09/borrowed-timeout-manifest.json`. Immutable binaries remain in the execution
workspace and are not bundled. The experimental and restoration commits remain
reachable together; this proof does not ship the rejected runtime change.

The archive also retains ten exact original/fixture/candidate/restored source
snapshots and a 319-file candidate input inventory. The inventory records the
pinned tree; it does not assert that every listed source was compiled. Frozen
benchmark inputs are bundled, and other library inputs remain retrievable from
the candidate Git commit. The packaging inspection distinguishes tool versions
observed afterwards from the original measurement and build records.

After extracting the proof into a directory, verify the raw child records and
profile totals without running a compiler or benchmark:

```sh
python3 EXTRACTED/work/audit-borrowed-timeout-proof.py EXTRACTED
```

This independent audit enforces captured argv and child iteration agreement,
reconstructs every median, verifies the additive clock correction and checks the
110 selected all-feature tests using the unique suite logs. Duplicate driver
logs do not inflate the test count.

The archived transparent wrapper preserves original arguments and byte streams,
including nonzero and signaled child status. It changes no benchmark source or
collector threshold. Reproduction uses separate capture directories, the real
Valgrind/strace paths, source labels and the frozen binaries:

```sh
devloop run --cells rpc.prost.server_stream --iters 200 --repeats 3 --out REPORT.json
devloop compare --rpc --baseline BEFORE.json --current AFTER.json
```

Check the targeted-win minimum independently from comparator status. Sampled
flamegraphs and allocation-stack tracing remain unavailable for the tool/runtime
reasons retained in [the original attribution](sv-09.md); none is fabricated.
