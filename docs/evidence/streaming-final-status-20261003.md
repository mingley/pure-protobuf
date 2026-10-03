# Streaming response completion requires grpc-status

The client now reports `Unknown` with `missing grpc-status` when a body-bearing
server-streaming or bidirectional response reaches clean HTTP/2 EOF without
final gRPC status. It continues to deliver preceding messages before reporting
the terminal error. Valid Trailers-Only status, explicit trailers, metadata,
malformed-frame errors, message limits, and native request half-close retain
their existing behavior. Binary logging records the local missing-status error.

Qualified candidate: `b8f1803eba16a5742c419a71480fc3d306136473`.
The two shipping/test files are byte-identical on main `ead02304`:

- `pbrs-grpc/src/wire/frame_reader.rs`: `c019ceb259b7fc9d2c3911678cb55f1daf021399bca18c44ffbf1fe224a9c2ab`
- `pbrs-grpc/tests/streaming_final_status.rs`: `14f9b5843d1464aa08648eccc9eb022dc71e4b3d69cc2e4f41840557d04ac87f`

The raw-H2 fixture contains twelve finite cases. On the original implementation,
five missing-status cases fail their intended assertions and seven controls
pass. All twelve pass on the candidate under both Rust 1.88 and the supported
Rust 1.85 minimum. Tests include empty DATA, bidi completion, Trailers-Only,
explicit success/error trailers, malformed input, size limits, request
half-close, existing unary rejection, and admission-permit recovery.

| Actual candidate gate | Result | Raw record SHA256 |
| --- | --- | --- |
| Rust 1.88 focused fixture | 12 passed, zero ignored | `15c0a2226b1214379006b8ec143aef039c5cf79506957cf19d7997774889c222` |
| Rust 1.85 focused fixture | 12 passed, zero ignored | `f1db9ad0e8e2b955d729aa1d8259d8de6d87cf3ea410420fd6b767724366f45e` |
| Rust 1.88 affected transport suites | 69 passed, zero ignored | `a972659e3435e957ce3f1951b3990ee2744b36eb21b55f898019ca94a36dec7c` |
| Rust 1.85 feature-off library check | Passed | `188ebc7c9a60bfe52a417fed0c3329d6bb5a2db64c5fec15826178dca9c56d1e` |
| Rust 1.88 Clippy, library and fixture, `-D warnings` | Passed | `f7089e99fe10aeedfd44b4c407a178bbbf9c59b70421d4f390ba070e2b6960d7` |
| Rust 1.88 rustdoc, `-D warnings` | Passed | `55686dca4deb5aad7bcad7acea759e8bfffc7f118cc98b521d12a91fbdcb15bc` |
| Baseline/candidate locked normal dependency trees | Passed; complete text equal after replacing only the declared checkout root | `abb3bc93c4d320ef3a37feb83b0abc523d6383a89558f260e5636bb45798099c` / `c563f4d02334e02819e579459310f59b68f0e2e1d0af58a2eae38ef022a7fbca` |

The affected suites are `bidi_completion` (1), `binlog` (32),
`byte_budget_public` (1), `deadline_admission` (10), `gaps` (6),
`message_size` (7), and `rpc` (12). Their actual executed seven ELF files,
selected local providers, dep-info, fingerprints, build scripts and both
generated output directories are retained as 91 full files, 300,475,498 bytes,
with zero hash-only payloads. The manifest is
`13998399589304307a66be5b62348eeb4f1b3489d96a1ec409ddc6fc84b8605f`;
independent full-byte/mode and execution-link readback is `3ba3ad393261ea5e7e43b61774339b59da28c5990149231d1f685d019fbc4d9a`.
The separate focused-fixture runs retain their own actual provider/ELF payloads.

Commands use offline locked dependencies, one Cargo job, disabled incremental
compilation/debug information and a 900-second bound. Initial formatter and
capture-parser failures remain recorded with their corrected attempts. Source,
tools and helpers match before/after every accepted run; owned children are
reaped. Complete shared cache accounting preserves the fixed 2 GiB cap and
free-space floor. Legacy-cache recovery has its own explicit admission failure
and does not retroactively qualify earlier overflowing runs.

This qualifies the bounded candidate correctness change. Combined-main checks
are a separate cohort. The added private boolean changes stream layout; its
size and performance are unmeasured. No benchmark threshold, dependency,
security policy, default transport option or performance claim changes. Late
synchronous decode deadlines, compatibility-producer cancellation, soak and
resource recovery, and broader streaming performance work remain open.
