# Native caller atomic-admission qualification

The isolated caller fix waits for authoritative HTTP/2 capacity before enqueueing request HEADERS. Native and opaque callers check local cancellation/deadline before each possible admission poll and refresh the owned relative timeout outside the stream mutex. The original raw-peer reds remain archived unchanged. This repair uses the reviewed backend primitive; it adds no pool semaphore, connection counter, retry rule, body task before admission, protocol reset change, or new unsafe code.

Native requests retain the existing 20 ms timeout formatting policy and avoid encoding the first outgoing timeout twice. Opaque requests use exact remaining time. A local timeout keeps a healthy connection available. The new regressions verify native four-shape timeout/cancel/drop, SETTINGS zero/grow/shrink, stream release, abort visible before the next capacity poll, same-connection reuse with no consumed stream ID, timeout refresh, and the native 20 ms boundary. Seven opaque cases use four path labels with one counted HTTP body fixture; the actual generated Tonic suites below provide separate shape/gzip evidence.

## Source identities and results

Every ordinary capture retains actual tool versions/hashes, source hashes, argv, environment, stdout/stderr, exit, and before/after guards. Direct test launches also guard the Cargo-selected ELF. External consumer runs retain direct Cargo.toml/Cargo.lock/main.rs copies before and after; packaged runs additionally guard unpacked source members.

| Source | Completed scope | Result |
|---|---|---|
| `2826b84b368027b625e91ed2db99d6d68b8537dc` | Initial actual Rust 1.88/Tonic broad suite: 17 new + 248 existing integration + 544 library tests | **809 unique passes**, zero failed tests, two declared ignores. Full unchanged lifecycle campaign passes 13 cases in 149.17 s. |
| `cc4db05383776ad39e8af34dee12a164b35ffcf2` | Actual Rust 1.88 all-target/all-feature Clippy with `-D warnings` | Exit 0; source/tool guards unchanged. |
| Same `cc4db053` | Actual Rust 1.85 default library 526, new deadline 10, original retry 18, preface 10 | **564 executions pass**, zero failures, one inherited HPACK ignore. |
| Same `cc4db053` | Default actual Rust 1.85 debug native interop client/server build | Exit 0; immutable workspace ELF copies supplied to the coordinator's fresh pinned-Go campaign. |
| `d6c4ddde5b3079473c0ec431044c7de57c4c372d` | Helper-only merge; all Rust/manifests/locks unchanged from `cc4db053`. Actual Rust 1.88/Tonic library 544, deadline 17, retry 18, preface 10 | **589 executions pass**, zero failures, one inherited HPACK ignore. |
| Same `d6c4ddd` | Fresh path and normalized-package consumers: default/Tower on actual 1.85, Tonic on actual 1.88 | **All six pass**. Each lock has 105 registry tuples, all in the provider's 166; indexmap remains 2.14.0. No backend patch. |
| Same `d6c4ddd` | Locked offline Cargo 1.99 package, no verification; verified by the fresh package consumers | 71 core / 286 grpc members. All **54 backend Rust files** and all 66 backend members, including licenses and provenance overlays, match source bytes. |
| Same `d6c4ddd` | WakeGuard replay inside the actual unpacked versioned grpc crate with explicit preserved old bytes | Exit 0; direct/package guards unchanged. |
| `52b9a92152e40ba2e54060c9a341d9622e7e00a9` | Necessary generated-test lock edge normalization; no Rust, manifest, schema, workload or tuple changes. Actual Rust 1.88 locked/offline generated Tonic tests | **8 client + 19 server executions pass**, zero failures/ignores. Actual shapes, gzip, deadlines/reset, mTLS, UDS and layers are covered. |

Repeated executions across sources/profiles are not combined into a larger distinct-test count. The generated cap opt-out tests qualified by another lane are a separate fixture and are not included in these 27 executions.

The normalized test lock adds only seven already present dependencies to its local pbrs-grpc node. All 151 package identities, all 146 registry tuples/checksums and every other node edge stay unchanged; its indexmap 2.14.2 pin stays unchanged. The root lock remains SHA256 `1f3f8fa2f28546a2d5c90f186736522e18bc1ddc9cdd0686f5dbc63cc84594d5`. Exact before/after bytes and the independent edge audit are in [the lock evidence](../native-caller-lock/audit.json).

The coordinator separately ran the immutable `cc4db053`/Rust 1.85 binaries against the pinned Go peer. Plaintext and TLS each record 46 passes plus eight explicit unsupported Go compression rows; mTLS records 18 native passes and 36 unsupported Go rows, with its required aggregate intentionally failing closed. This pack does not turn native mTLS self-tests into cross-language mTLS qualification. The coordinator owns that independently sealed raw campaign.

## Retained failures and limits

The first invalid provenance helper called cargo-clippy as a version probe and produced E0514 before launching the planned strict gate. Its full streamed stderr was truncated and not retained; the artifact audit proves the prior test ELFs were unchanged. Later strict captures retain the inherited 12 formatting-lint failures, the new opaque fixture's unreachable branch, and the unavailable newer Clippy lint. The opaque fixture now has only its two valid states; the component task waker now has an observable counter. Old/new bytes and additive replay records preserve the original imported topology mapping.

A generic 45-second lifecycle runner timed out during the 4,000-connection campaign. The same source/ELF completes under a documented 900-second campaign bound; individual 2 s/5 s assertions, schedules, and workloads are unchanged. The inherited HPACK ignore and `current_h2_resource_smoke` ignore remain unqualified. A missing-PROTOC preflight and disk-reserve preflight launched no compiler. Cargo 1.88's unavailable `package --registry` flag and the first stale generated lock both retain exit 101; packaging then uses the topology workflow's stable Cargo 1.99 in a separate package-only target, and the generated lock receives only the audited seven-edge correction. One read-only proof helper initially mistook archived `.rs` documentation copies for compiler changes; the corrected compiler-input audit excludes evidence paths. Its complete initial streamed stderr was not retained.

Pre-poll clock/watch checks remain cooperative and outside the backend mutex. Cancellation concurrent with already completed admission retains protocol-valid HEADERS/reset behavior. No FIFO or universal fairness guarantee is asserted. Lazy connect, SETTINGS-bounded eager connect/from_io, wait-for-ready, deadlines, generation isolation, limits, pool guards and body startup ordering remain unchanged. Historical topology Miri/status/package evidence remains at its original source identities; the caller adds no unsafe code.

This is bounded correctness evidence pending coordinator review. It makes no performance, full-profile interoperability, cross-language mTLS, or long-soak claim. Preserved executable/rlib artifacts remain workspace/hash references and are excluded from git blobs. Completed owned caches were reclaimed only after exclusive compiler checks, all-byte manifests and executable hash verification. The last generated-source/fingerprint set is retained in the raw archive; prior cache manifests retain hashes, while the selected source/tool snapshots support regeneration.

## Portable audit

Run `python3 docs/evidence/deadline-admission/native-caller/check-artifacts.py`. It verifies all outer files and archive/member hashes, seven self-contained compiler/test-input source snapshots, 60 capture records, captured stream hashes, source/input guards, final test counts, six consumer lock pin sets, package members, unchanged compiler inputs across the helper-only merge, and the exact test-lock edge correction. It launches no compiler or runtime. `artifact-sha256.json` pins the outer proof files; `members-sha256.json` pins all inner members. `raw.tar.gz` retains full capture logs/metadata, helper versions, input copies, generated/fingerprint inputs, normalization audits and the actual `.crate` files.
