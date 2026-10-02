# CL-08 Stage 1: isolate the default cold handshake future

This isolated experiment changes only the default pooled dial await to
`Box::pin(handshake(...)).await`. At the pinned unoptimized test profile,
`acquire` and `grab_in` each shrink by 5,688 bytes. The release streaming primary
subsequently showed lower instruction/byte counters, but the original comparison
failed and unchanged syscall/futex controls failed stability. This is a
**measurement gap**, not a qualified candidate win. The stage is not qualified
for shipping and CL-08 remains open.

The baseline source is
`ea3089b69fc20d64048973bc5f787dda1d266988`: cf3 shipping source plus CL-07
regressions and the dedicated [layout probe](cl-08-cold-stage-1/probe.patch).
The candidate is `6a4d06200ade81eb4831db6cecf6f05d539cc5c8`, whose entire
production change is the [single cold await](cl-08-cold-stage-1/cold-box.patch).
It does not inherit rejected contiguous-codec source `2321d736`.

## Pinned layout observations

Both commands use rustc 1.99.0 (b940084d7), x86_64-unknown-linux-gnu, the unchanged
locked dependencies, the unoptimized test profile and `prost,copy-counts`.
The futures are constructed but never polled; there is no network experiment.

```sh
source /workspace/pure-protobuf/work/toolchain/env.sh
export CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/workspace/scratch/work/cl07/target
cargo test --locked -p pbrs-grpc --features prost,copy-counts \
    --lib cl08_future_sizes -- --nocapture
```

| Future or wrapper | Before bytes | After bytes |
|---|---:|---:|
| Default `ChannelInner::acquire` future | 8,032 | 2,344 |
| `Channel::grab_in<TokioRuntime>` future | 8,504 | 2,816 |
| Cold `handshake` future | 7,784 | 7,784 |
| Public erased `Call<Response<HelloReply>>` wrapper | 32 | 32 |

The [source/log-bound report](cl-08-cold-stage-1/size-report.json) and raw
[before](cl-08-cold-stage-1/before-size.log)/
[after](cl-08-cold-stage-1/after-size.log) results retain the exact observations.
The public `Call` stores a pointer to an erased boxed future. Its 32-byte wrapper
size does not describe that heap allocation; the underlying RPC future size is
explicitly **not measured**. These test-profile layouts are not a claimed
release allocation delta, CPU reduction or wall-time improvement.

## Semantics and expected costs

The default warm pool-hit branch returns before reaching the new box. Dial
start telemetry, observer order, slot lock release, generation checks, sender
readiness and peer stream-cap waits remain in their original order. The cold
branch creates the same handshake future behind an owned pinned pointer. The
existing handshake timeout, cancellation/drop, wait-for-ready retry/backoff,
TLS, lazy connection and deadline races remain unchanged.

The source introduces one box only when entering this default cold-dial branch;
its observed test-profile future layout is 7,784 bytes. That is a source/layout
cost hypothesis, not an allocator-counter result. Warm default hits do not
enter this allocation site. Optional resolved LB acquire methods are untouched;
no warm-LB boxing allocation or new private LB dispatcher is added. The large
first-stage shrink warrants measuring this variant before considering that
separate tradeoff.

The proposed benefit is smaller enclosing RPC futures and their existing boxed
allocation/copies. Confirming it requires source-pinned release allocation and
instruction controls, including cold/TLS costs. Only the single release streaming primary and
unchanged diagnostics below ran; the original full 20-cell, all-ledger and
cold/TLS performance requirements are not run for this stage. The rejected earlier codec experiment's failed original
comparison, 58 remaining ledger byte losses and 12 blocked map profile rows
remain separate evidence and do not become passes here. No thresholds, protocol
limits, defaults, benchmark collectors, unsafe code or dependencies change.

## Correctness qualification

All **507 scoped tests passed**, with zero failures or ignores: 439 library,
10 CL-07 raw-peer, 13 lifecycle, one timeout parser, 18 retry/deadline/cancellation,
and 26 TLS/mTLS tests ([raw log](cl-08-cold-stage-1/cl08-cold-semantic-gates.log)).
The lifecycle suite includes the 5,000 recorded seeded scenarios across FromIo,
TCP, TLS, mTLS and UDS; no test schedule or assertions changed.

```sh
cargo test --locked -p pbrs-grpc --features prost,copy-counts --lib \
    --test cl07_preface --test tls --test retry_safety \
    --test regress_timeout --test lifecycle -- --test-threads=1
cargo clippy --locked -p pbrs-grpc --features prost,copy-counts \
    --lib --tests -- -D warnings
cargo fmt --package pbrs-grpc -- --check
```

Strict [library/test Clippy](cl-08-cold-stage-1/cl08-cold-clippy.log) and
[formatting](cl-08-cold-stage-1/cl08-cold-format.log) passed. The
[first Clippy attempt](cl-08-cold-stage-1/cl08-cold-clippy-first-os28.log) failed
before compilation with host OS28 (no space for a target fingerprint). Available
space recovered; completed owned debug executables were then pruned, retaining
[cleanup accounting](cl-08-cold-stage-1/cl08-cold-pruned-completed-debug.json).
The unchanged source retry passed. This infrastructure failure is retained,
not counted as a semantic pass.

The jobs=1 debug build reused the owned CL-07 target. Owned artifacts were
1,647,121,855 bytes after semantics and 1,614,049,333 bytes after strict checks,
below the assigned 2 GiB limit. The two immutable earlier release binaries and
release dependency cache were untouched by debug cleanup. The subsequent release/capture details below retain every original failure.
Candidate integration remains unqualified until the original performance
requirements are satisfied.


## Release primary and unchanged-counter diagnostics: unqualified

The clean actual release build source is
`ab62da78eb48b4e3cdc6dbdd44555a9dfe656cf3`, containing the pinned Stage 1 source
and its layout/semantic evidence. The [build record](cl-08-cold-stage-1/cl08-cold-release-build.json)
pins the immutable candidate SHA256
`0a492e5a24b49b25ff57cd0a1f807cb73b6c500fb6f7f0aaa9159c150264f5b4`
and exact cf3 baseline SHA256
`73fa59c733f0637b22ce8d58f035aa45cb13f0b7efc4c8e9bb4cdddcef54e0b9`.
Release thin LTO/codegen-units=1/opt3, dependencies, collector, workload and tool
flags match the baseline. The first build lacked the new worktree's pinned
protobuf source symlink and failed in v4_tat; its
[raw failure](cl-08-cold-stage-1/cl08-cold-release-build-first-missing-protobuf.log)
is retained. Correcting that setup with the verified protobuf35.1 source pin
allowed the unchanged source [retry](cl-08-cold-stage-1/cl08-cold-release-build.log)
to build in 6m22s. Owned target bytes after release were 1,578,670,979 (<2 GiB).

The coordinator granted a quiet single-row original-N primary capture, with
other compiler trees paused. The unchanged frozen collector ran
`rpc.prost.server_stream` at N=200/400, three repeats, identical preparation
N=400 and immutable launch/post hashes. Every CG process succeeded; all raw CG
[before](cl-08-cold-stage-1/cold-primary/before.raw.jsonl)/
[after](cl-08-cold-stage-1/cold-primary/after.raw.jsonl) records are retained.

| Original release streaming primary | Before | After | Change |
|---|---:|---:|---:|
| Instructions/RPC | 158,677.43 | 147,353.745 | −7.136% |
| Allocations/RPC | 57.025 | 57.025 | 0% |
| Allocated bytes/RPC | 62,632.64 | 56,992.64 | −9.005% |
| Whole-child traced syscalls/RPC | 30.635 | 31.01 | +1.224% |
| Whole-child futex-family calls/RPC | 14.81 | 15.315 | +3.410% |

The **original comparison exits 1, failed**, because futex-family calls exceed
the unchanged 2% RPC limit ([raw verdict](cl-08-cold-stage-1/cold-primary/compare.log)).
The instruction reduction meets the original numerical primary target, but the
complete verdict does not qualify this candidate. The byte delta is 5,640/RPC,
not the test-profile layout delta of 5,688. The actual erased RPC body remains
unmeasured; aggregate counters alone do not establish its exact allocation site.

The coordinator then approved **additional diagnostic controls**: two reports
from the same immutable baseline and two from the same immutable candidate,
each retaining the original N200/400×3 inputs and collector. These do not
supplant, average with, or select around the original failed pair. Raw reports,
children and selfcomparisons live in
[unchanged diagnostics](cl-08-cold-stage-1/cold-self-controls/metadata.json).

| Unchanged report pair | Instructions drift | Syscall drift | Futex-family drift | Frozen selfcompare exit |
|---|---:|---:|---:|---:|
| Baseline → same baseline | −0.402% | −3.525% | −2.749% | 0 |
| Candidate → same candidate | −0.725% | +3.785% | +3.826% | 1 |

Allocation counts and bytes are identical within each unchanged pair.
Instruction drift stays below the existing 1% unchanged-control rule; both
syscall/futex pairs exceed that rule in absolute value. The baseline's frozen
selfcompare exits 0 because those metrics decreased, while the unchanged
candidate's selfcompare exits 1 on both increases above 2%. These observations
show counter instability even without a source change. The coordinator accepted
the result as a **measurement gap/negative qualification**, retaining all failures
and rejecting shipping/full-20 qualification. No metric threshold changes.

The [independent validator](cl-08-cold-stage-1/validate-controls.py) reconstructs
all original and diagnostic primary medians from **36 raw CG children**, checks
all source/row/N/preparation/binary bindings and retains every
[full graph](cl-08-cold-stage-1/full-graphs.tar.gz).
The [numeric summary](cl-08-cold-stage-1/cl08-cold-controls-summary.json) includes
absolute 1% unchanged drift outcomes. All [capture scripts](cl-08-cold-stage-1/capture-self-controls.py)
are diagnostic workspace artifacts; the shipping collector is unchanged.
Wall numbers/CVs are noisy shared-host diagnostics only. There is no qualified
CPU, latency, throughput, syscall or lock claim. Full20, cold/TLS and all-ledger
qualification for this cold stage remain `not_run`; the earlier codec stage's
58 losing byte rows and 12 blocked maps remain open. Optional-LB boxing is not
implemented. The candidate source/binary stays isolated for a potential separate
stable campaign; no runtime merge or source revert is performed by this worker.


The subsequent [read-only collector audit](cl-08-cold-stage-1/collector-scope-audit.md)
clarifies that syscall/futex medians come from separate whole-process strace
children, not Callgrind or the allocator window. The six reports contain 36 CG
children with raw evidence plus 18 strace children whose individual summaries
were not retained. That raw trace provenance gap prevents independent breakdown
or median reconstruction of those secondary counters. It does not erase their
failed aggregate verdicts. The completed release cache was explicitly cleaned
at the coordinator's request after verifying no active compiler used it;
[848 fingerprint hashes](cl-08-cold-stage-1/cl08-cold-explicit-target-clean.json),
source/gate/build/profile records, and both immutable executable hashes were
preserved. [Cargo clean](cl-08-cold-stage-1/cl08-cold-explicit-target-clean.log)
removed 1.6 GiB; the isolated candidate source remains unreverted.
