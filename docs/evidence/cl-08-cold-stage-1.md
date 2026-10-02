# CL-08 Stage 1: isolate the default cold handshake future

This isolated experiment changes only the default pooled dial await to
`Box::pin(handshake(...)).await`. At the pinned unoptimized test profile,
`acquire` and `grab_in` each shrink by 5,688 bytes. Release allocation,
instruction and latency effects are not measured here; the stage is not
qualified for shipping and CL-08 remains open.

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
instruction controls, including cold/TLS costs; those measurements are not run
in this report. The rejected earlier codec experiment's failed original
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
release dependency cache were untouched by debug cleanup. No release/LTO build
or quiet timing was started for this stage. Candidate integration remains
unqualified until the original performance requirements are satisfied.
