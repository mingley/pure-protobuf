# TC-30b1 identity message caps — 2026-10-02

Native message-cap overrides now restrict identity-coded messages from unchanged
tonic-generated services mounted through `TonicServer`. The final correctness
source is `d4a3d739e4063c3db0ee59724f6d0508ba4144fb`, based on
`a5410636d0c508b87f7cd4dd2a430bfc27821880`. This is the identity-only portion
of TC-30b. Compressed-message guards, native gzip policy and retained-byte budget
accounting remain unresolved and explicitly rejected when configured. No
comparative performance, default-path allocation measurement, dedicated-host
campaign or production qualification was performed.

## Behavior and boundaries

Only directions whose native cap differs from the default select the cold
framing guard. The existing `IncomingBody` and default response drain remain
unchanged. The guard and its framing state live in the boxed override future
and bodies; the default branch does not allocate those boxes. This is a source
property, rather than an independently measured performance result.

The guard retains at most five nonempty original `Bytes` prefix views until the
complete five-byte frame header is known and validated. It copies those five
control bytes into a scratch header, then forwards the original prefix views
and payload slices. It neither protobuf-decodes nor copies the message payload.
Each identity message is checked against the native directional cap before any
of that message's prefix is forwarded. Previously admitted complete messages
remain deliverable when a later message fails. Native caps cannot enlarge a
stricter private tonic codec cap, and stream totals are not treated as a single
message cap.

| Condition | Outcome |
|---|---|
| Default native caps/compression | Existing opaque forwarding; tonic owns its codec caps and compression. |
| Changed cap, identity message over that cap | `RESOURCE_EXHAUSTED`, without forwarding that message's prefix. |
| Changed inbound cap, nonidentity encoding header | `FAILED_PRECONDITION` before Tower readiness or business dispatch. |
| Changed outbound cap, nonidentity response encoding | `FAILED_PRECONDITION` after the handler returns, before response headers. Unsupported output cannot be determined before business dispatch. |
| Changed cap, compressed frame flag | `FAILED_PRECONDITION`; no inflate guard or inflated-size claim. |
| Invalid frame flag or incomplete frame at EOF/trailers | `INTERNAL`; one error then fused EOF, with producer/state dropped. |
| Explicit producer error | Original tonic status, details and metadata retained. |
| Native compression configuration, finite byte budget, response hooks, binary log, observer or grpc-web | Existing unsupported-policy rejection before readiness/dispatch. The explicit send-buffer setter's implicit finite-budget edge remains rejected. |

The existing incoming body releases HTTP/2 credit when DATA is handed to the
cold guard. Withholding the first five bytes therefore does not deadlock a
one-byte receive window. This does not account tonic's private decoded buffer
or producer-owned response memory. Deadline/reset guards remain outside the
body wrapper and drop pending handlers/readers/producers while preserving
native admission recovery.

The implementation spans the tonic adapter/private helper, native policy
fixture and unchanged-generated standalone fixture. No dependency, manifest,
lock, unsafe-code or shipping-default change was made. The two old blanket
identity-cap rejection entries were replaced with actual cap tests; every other
unsupported-policy fixture entry remains.

## Source-pinned gates and retained attempts

The final source passed six private framing tests, 22 native TC-30a/b transport
tests and all 29 standalone generated-consumer tests. Native and standalone
strict Clippy passed with `-D warnings`. Strict all-feature rustdoc, feature-off library compilation and normal dependency-tree checks passed. The feature-off normal graph contains no tonic, Tower, Hyper or HTTP body packages. Root format, owned-file format, diff and unchanged manifest/lock checks passed.

The six framing tests cover all 4,096 segmentations of an independently
specified empty-plus-three-byte message stream, original allocation spans,
per-message/disabled caps, invalid flags/large claims, truncated EOF/trailers,
error fusion/drop and explicit producer status details/metadata. Native tests
exercise cap boundaries, one-byte windows, empty/coalesced DATA, all framing
failure stages and deadline/reset during partial inbound/outbound prefixes.
Two successive RPCs under a one-RPC admission limit exercise recovery.

The generated fixture adds identity success and inbound/outbound rejection for
all four RPC shapes, plaintext and verified mTLS, with native security
interceptors, ordinary Tower layers and exact native/tonic connection facts.
It preserves existing gzip/default-policy and private tonic-cap proofs. Its
unchanged `include_proto!("routeguide")` output is archived; the two observed
build profiles produced the same SHA-256,
`98b1dbf8d0c5bd1f5df08b796197378db1b3c458ad079b0c7f3f64e8b1d48d87`.

A broader native regression run at the earlier clean source
`add6b6729a9f1f8d869d48969fb2f68f20e33875` passed 1,219 tests across nine
targets: hostile 41, message-size seven, RPC 12, serving 1,100, TLS 26, identity
10, tonic server two, TC-30a 12 and TC-29 nine. That result remains pinned to
`add6b672`; it is not relabeled as a final-source broad run. The later source
changes add checked prefix indexing and narrowly documented fixture assertions.
The affected framing/native/generated gates were rerun at the final source.

| Retained attempt | Source | Result |
|---|---|---|
| New cap regression before implementation | `a5410636` plus archived source-rejected patch | Exit 101: actual `FAILED_PRECONDITION` (9), required `RESOURCE_EXHAUSTED` (8). This records the former documented unsupported policy, rather than a misreported old bug. |
| Intermediate draft native/framing checks | Uncommitted draft, not a final source qualification | Seven native and five framing passes; tonic-only framing build retained an existing unrelated unused import warning in `copy_counts.rs`. |
| Preliminary clean native/consumer checks | `add6b672` | 21 tonic-only native and 29 generated passes; all-feature six framing and 1,219 broad native passes above. Repeated tests are not added to final counts. |
| First strict native Clippy | `add6b672` | Exit 101: three variable indexing/slicing errors in the cold prefix guard. |
| Second strict native Clippy | `ccb72c18` | Exit 101: 11 fixture-helper `expect_used` diagnostics. |
| Final strict native/generated Clippy and affected tests | `d4a3d739` | Exit 0. Checked access returns `INTERNAL` for impossible prefix state; five fixture helpers have item-level assertion expectations, and response construction propagates its error. No broad shipping lint suppression. |
| Whole standalone consumer format | `d4a3d739` | Exit 1 solely in unchanged `build.rs` and `tests/example_ports.rs`; their diffs from the base are empty. Owned generated/native files and root-wide format pass. |
| First individual generated format invocation | `d4a3d739` | Incorrect edition 2021 rejected an existing let-chain; rerun with manifest edition 2024 passed without source changes. |

Tonic-client-transport independently reviewed `add6b672` and the checked-access
followup `ccb72c18`, and root reviewed final production source `d4a3d739`.
They found no source blocker and performed no independent executions for those
reviews. Full byte-budget lifetime/compression/default-performance qualification
remains outside this leaf.

## Reproduction and provenance

The [handoff](tc30b1-identity-caps-20261002/handoff.json) records exact commands,
source, toolchain, lock/manifest pins, results and original paths. The
[raw archive](tc30b1-identity-caps-20261002/raw-artifacts.tar.gz),
[member hashes](tc30b1-identity-caps-20261002/member-sha256.txt) and
[archive manifest](tc30b1-identity-caps-20261002/archive-manifest.json)
retain 67 artifacts, including failures and original/final executable
fingerprints. Archive SHA-256: `079b3aeb90fdb243230a73aac8f9f293898ba5798a0307b34ca742eef29e3863`.

Tools were Rust `1.99.0 (b940084d7 2026-09-28)`, Cargo
`1.99.0 (5f94df478 2026-08-27)` and protoc `35.1`. Root lock SHA-256 is
`143bd380614c258637da68df0e839157955b0c294a5faf563e927034d557a76e`;
standalone tonic-consumer lock SHA-256 is
`54ae424e91c4032d97306aea5464afe7a3d8f0137d65973023e15d3c14a70c19`.
The native and standalone graphs intentionally retain different locked
`tokio-rustls` versions (0.26.4 / 0.26.6) with tonic 0.14.6.

All Cargo compilation used one job and an isolated cache, with debug information
disabled for final correctness gates to fit the 2 GiB allocation. Other agents
compiled concurrently; there was no quiet measurement lease. Completed test
executables were fingerprinted before reclaiming those inactive cache outputs.
The earlier debug cache was also fingerprinted and cleaned. The largest observed cache was 2,083,182,099 bytes, below 2 GiB; the final observed cache was 1,957,385,710 bytes after reclamation.

From a clean checkout at the exact final source, with the pinned tools:

```sh
export CARGO_BUILD_JOBS=1
export CARGO_TARGET_DIR="$PWD/work/tc30b1-correctness-target"
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
cargo test --locked -p pbrs-grpc --all-features --lib tonic_server::opaque::tests -- --test-threads=1
cargo test --locked -p pbrs-grpc --all-features --test tonic_opaque_transport --test tonic_service_transport -- --test-threads=1
cargo test --locked --manifest-path tests/interop/tonic/Cargo.toml --all-targets -- --test-threads=1
cargo clippy --locked -p pbrs-grpc --all-features --lib --test tonic_opaque_transport --test tonic_service_transport -- -D warnings
cargo clippy --locked --manifest-path tests/interop/tonic/Cargo.toml --all-targets -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --locked -p pbrs-grpc --all-features --no-deps
cargo check --locked -p pbrs-grpc --no-default-features --lib
cargo tree --locked -p pbrs-grpc --no-default-features --edges normal
cargo fmt --all -- --check
rustfmt --edition 2024 --check tests/interop/tonic/tests/transport_services.rs
git diff --check
```

The session's exact target/toolchain paths are retained in the handoff. Verify
archived raw members from the repository root:

```sh
mkdir -p work/tc30b1-raw
tar -xzf docs/evidence/tc30b1-identity-caps-20261002/raw-artifacts.tar.gz -C work/tc30b1-raw
cd work/tc30b1-raw
sha256sum -c ../../docs/evidence/tc30b1-identity-caps-20261002/member-sha256.txt
```

To reproduce the original intended rejection failure, use a separate checkout
at `a5410636d0c508b87f7cd4dd2a430bfc27821880`, apply the archived
`raw/source-rejected-before-fix.patch`, and run:

```sh
cargo test --locked -p pbrs-grpc --features tonic --test tonic_opaque_transport identity_outbound_cap_rejects_oversize_without_emitting_prefix -- --exact --nocapture
```

That source-plus-patch is expected to fail; the same assertion passes on the
final source. These gates qualify the bounded identity-cap behavior described
here, not compressed opaque bodies, retained byte budgets, native compression
preferences, default transport speed or production readiness.
