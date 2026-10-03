# TC-30b2a finite inbound gzip caps — 2026-10-03

Changed, finite native decoding caps now restrict gzip requests on unchanged
`TonicServer` services. The final source is
`f8ce573279228ff0a76ee48daa176aadb685f729`, based on
`76d43b7d064ee63d2231fb0be2e31128b0f165b0`. Stable correctness and strict gates
ran on `22a77333c409a8949f7c030a94e084ca1e17087d`; the only subsequent change
is a private comment clarifying optional-header work. The archive verifies that
exact comment replacement. Rust 1.85/1.88 checks ran on the final source.

This completes the inbound finite-cap leaf, not TC-30b's full compression or
byte-budget policy. There was no release build, performance capture, default
allocation measurement, RSS qualification, dedicated-host campaign or production
soak. No dependency, lockfile, manifest, unsafe code or shipping default changed.

## Behavior and limits

The default forwarding branch and the existing identity guard remain unchanged.
Only a changed, finite decoding cap plus a `grpc-encoding: gzip` request selects
the boxed gzip guard. It checks the declared encoded message length against the
native decoding cap, inflates into an 8 KiB scratch buffer to check the same cap
against inflated length, and verifies the first member's checksum/size footer.
It withholds that message's original prefix and payload views until validation
and the complete declared encoded boundary succeed, then replays those views.
It performs no protobuf decode or compressed-message coalescing. Tonic inflates
again for its own decoding; this double inflation has not been performance
qualified. A stricter private tonic codec cap remains effective.

Both the existing native inflater and unchanged tonic generated decoder consume
the first gzip member and ignore a second member or garbage inside the declared
encoded payload. The characterization patches and passes precede this guard.
The guard preserves that behavior, while still requiring all declared tail bytes
to arrive before forwarding. An incomplete footer/tail remains subject to EOF,
trailer, reset and deadline handling. Earlier admitted messages remain deliverable
when a later message fails.

Retained logical state includes the capped encoded payload, up to five original
prefix views, and an already-received coalesced DATA tail. The number of nonempty
payload views cannot exceed the encoded byte count, but vector capacity, pinned
`Bytes` backing allocations, the inflater dictionary/state and tonic's private
`BytesMut` retention are separate. This is not finite byte-budget accounting.
Flate2 can allocate a declared `u16`-length FEXTRA field before all bytes arrive;
optional header fields have existing limits of up to 65,535 bytes. The guard's
8 KiB quota limits newly consumed input and produced scratch output per call,
not every backend CPU/allocation operation: FHCRC can checksum a completed
optional field in one call. Input-quota exhaustion and positive inflate output
yield; empty DATA uses Tokio cooperative progress. These are bounded operations,
not a measured fairness or total-memory guarantee.

| Condition | Outcome |
|---|---|
| Default native caps/compression | Original opaque forwarding and tonic codec policy. |
| Changed finite inbound cap, gzip header, frame flag 0 or 1 | Encoded/inflated checks before that message's original prefix is forwarded. |
| Encoded claim or inflated payload exceeds native decoding cap | `RESOURCE_EXHAUSTED`, with no prefix of the rejected message. |
| Invalid gzip, checksum/size footer, or incomplete frame at EOF/trailers | `INTERNAL`, one error then fused EOF; retained state/source dropped. |
| Explicit producer error | Original tonic code, details and metadata retained. |
| Changed unlimited decoding cap plus gzip, or other unsupported request encoding | Existing `FAILED_PRECONDITION` before Tower readiness/dispatch. |
| Changed outbound cap plus nonidentity response encoding | Existing `FAILED_PRECONDITION` after the handler returns, before headers. |
| Native compression preferences/level, finite byte budget, hooks, binary log, observer or grpc-web | Existing unsupported-policy rejection before readiness/dispatch, including a send-buffer setter that implicitly enables a finite budget. |

Native authentication and verified connection information remain in effect.
Tonic streaming handlers receive a stream before reading messages, so input
rejection does not imply zero handler entries for those shapes. The generated
all-shape negative fixture explicitly records two streaming handler entries;
invalid decoded messages are never delivered. Native authentication rejection
records zero business-handler entries.

## Reproduction and results

Use the pinned source, source
`/workspace/pure-protobuf/work/toolchain/env.sh`, and set
`CARGO_BUILD_JOBS=1`, `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=0`,
`CARGO_PROFILE_TEST_DEBUG=0` and an isolated `CARGO_TARGET_DIR`.
The exact commands, environments, raw outputs and exits are archived.

| Gate | Source | Result |
|---|---|---|
| Private identity/gzip framing tests | `22a77333` | 16 passed; 453 filtered, not executed. |
| Native hostile/message-size/TLS/opaque/TC-30a/TC-29/tonic-server tests | `22a77333` | 117 passed: 41 + 7 + 26 + 20 + 12 + 9 + 2. |
| Standalone tonic consumer all targets | `22a77333` | 34 passed: services 19, clients 8, examples 4, binary 3. |
| Native strict Clippy | `22a77333` | Library plus `tonic_opaque_transport`, `tonic_service_transport`, `tonic_transport`, `tonic_server` passed with `-D warnings`. |
| Standalone consumer strict Clippy | `22a77333` | All targets passed with `-D warnings`. |
| Strict rustdoc | `22a77333` | All-feature `pbrs-grpc` docs passed with `RUSTDOCFLAGS='-D warnings'`. |
| Root/owned-fixture formatting, whitespace, feature-off normal tree, unchanged manifests/locks | Recorded source pins | Passed. Feature-off tree contains no tonic/Tower adapter graph. |
| Rust 1.85 feature-off library check | `f8ce5732` | Passed, locked. |
| Rust 1.88 tonic-feature library check | `f8ce5732` | Passed, locked. |
| Whole standalone-consumer format check | `22a77333` | Failed only in unchanged `build.rs`/`tests/example_ports.rs`; inherited debt retained. Owned fixture format passed. |

The 167 final test executions exclude earlier characterization, preliminary and
repeat runs. They cover original `Bytes` allocation spans and mixed/coalesced
messages, zero caps, one-byte windows, encoded and inflated bombs, optional
headers/FHCRC/payload CRC/ISIZE, truncation/trailers, errors with details/metadata,
cooperative inflate progress, retained backing release during inflation/replay,
reset/deadline recovery during partial headers/footers/ignored tails, all four
unchanged generated RPC shapes, stricter codec limits, native auth and verified
mTLS. Deadline fixtures retain 50 ms timeouts and the tiny-window fixture retains
its 2-second failure bound.

The required first red at the baseline plus `gzip-before-fix.patch` returned
status 9 instead of expected status 0 (exit 101). Native and tonic first-member
characterizations passed independently at the baseline plus their retained
fixture patches. Three preliminary passes lack a complete exercised source pin
and are excluded from final qualification; their logs and draft are preserved.
A later command typo used nonexistent `hostile_frames` and exited 101 before
compilation/tests; the corrected `hostile` target then passed. Neither failed
attempt was erased or converted into a pass.

Stable tools were Rust/Cargo 1.99, protoc 35.1 and Python 3.12.14. Exact versions,
lock/source hashes and separate native/standalone graphs are recorded; native
`tokio-rustls` 0.26.4 and standalone 0.26.6 remain distinct locked graphs with
tonic 0.14.6. All compilation used jobs 1 on the shared host. Twelve final ELF
and 87 generated-Rust fingerprints were saved before cleaning the inactive stable
cache, with no active owned compiler/executable process. Its last unique-byte
`du -sb` size was 2,094,737,318 bytes (below 2 GiB). The separate path-stat sum
counts hardlinks repeatedly; both the original record and corrected labels are
retained. The subsequent MSRV cache was 437,244,130 bytes at completion.

## Raw proof

The sibling artifact directory contains the raw archive, per-member hashes,
archive manifest and handoff records. `verify.py` checks every member and exit,
reconstructs the 167 final test executions, and validates the source/lock pins.
It checks evidence consistency, not performance or production qualification.
The parent TC-30b finite-budget/configured-compression/outbound-gzip work and
QG-06 production resource qualification remain open.
