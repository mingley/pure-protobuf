# Status history before November 5, 2026

This was the opening summary of [implementation status](../status.md) before
its November 5 cleanup. Each validation and measurement keeps its original
source revision and limits. For current navigation, use the
[status page](../status.md) and [benchmark results](../benchmarks.md).

The [November 2 follow-up](readiness-followup-20261102/README.md)
repairs invalid 32-bit generated defaults, rejects malformed descriptor names,
and makes RPC benchmark input reporting consistent across processes. The
record distinguishes native tests, i686 emulation and focused Miri checks.
The 32-bit layout repair costs four bytes per lazy string or byte wrapper;
measured x86_64 wrapper sizes remain unchanged. A separate gRPC change removes
redundant shared-buffer ownership work. Its isolated chunk-loop screen is
faster; end-to-end RPC gains and a new tonic/Prost ranking are unmeasured.

The [October 30 release checks](release-readiness-20261030/README.md)
prepare core 0.2.1 and adapter 0.1.0-alpha.3 candidates and retain earlier
transport, package and resource checks. All 11 required hosted CI jobs passed
at `a3570158`; later changes need checks on their own commit. These versions
remain unpublished. Hosted release provenance and sustained production
qualification remain open. The [latest runtime screens](rejected-runtime-candidates-20261030/README.md)
retain rejected map and repeated-field changes.

The [October 29 record](runtime-and-codegen-20261029/README.md)
measures `0a770792`: descriptor literals, generated repeated-tag runs and a
varint fast path. Lazy nested reads remain slower than Prost; owned sparse
parsing and non-LTO packed parsing regress against prior pbrs, while all 16
standard owned parse/full-read medians favor pbrs over Prost. The all-feature
workspace at `bc8cec38` passes 3,994 tests, with no failures and nine existing
ignores; its default conformance run records a clean source. Fresh owned
conformance uses a documented 13-binding overlay. Strict Prost comparisons
retain known differences. Full tonic/prost parity and an overall performance
ranking remain unproven.

The [October 28 record](optimizations-20261028/README.md) measures
source `ce0a06e5`, which repairs transport byte/token counter overflow and outbound
frame-length handling. Checked acquisitions never publish a wrapped value;
identity frames reject unrepresentable lengths before reservation or codec
execution. Encoding caps retain error precedence, and an earlier server reply
still arrives before a later error trailer. Fifteen budget tests, 21 framing
units and three framing integrations pass, with strict transport Clippy and
Rust 1.85 checks for the tracker change. Actual 32-bit execution and allocations
above 4 GiB were not tested.

The [October 28 benchmark follow-up](../benchmarks.md#correctness-and-rejected-candidates-october-28)
rejects the lazy enum, map iteration, generated-output ownership and both
borrowed-wire candidates after lifecycle regressions. That October 28 selected source
kept its earlier map, lazy-message and generator designs. Focused checks do
not establish a speed improvement.

The all-feature workspace run at `ce0a06e5` passes 3,992 tests, with zero
failures and nine existing ignores. `cf97c83b` changes one test closure to fix
a lint; the affected map suite and strict lint checks pass again. Rust 1.85,
message-presence and mixed-peer checks, owned/validated-lazy consumers, and
regeneration checks pass. At `cf97c83b`, required official conformance runs
twice and recommended conformance once: each passes 5,631 binary/JSON and 909
text cases, with zero failures or skips, maximum Edition 2023. Recorded inputs
match apart from Markdown documentation changes. Strict Prost comparisons
still report known behavior differences in both message profiles; their
known-differences checks pass. This is not full Prost equivalence.

The selected campaign completes 684 RPC captures. Large shipping endpoint CPU
medians give five wins, five losses and two ties against tonic; all four
default unary/pipeline rows favor native. Longer 8 MiB identity confirmation
still costs 1.494 times tonic client CPU and 1.654 times server CPU, with wide
pair ranges. Warm maps win five of 42 rows against standard HashMap. Lazy
depth-8/64 full reads cost 2.925/2.382 times Prost CPU, and the owned mixed
full read costs 1.095 times Prost CPU. Core source is unchanged; separate
binary layout and shared-host noise limit comparisons with earlier campaigns.

The [earlier measured core](decode-and-map-speed-20261027/README.md)
at `71422f00` reuses validated singular-child wire during lazy access, avoids backing
work for short map entries, and walks warmed unique maps without per-entry
index searches. It also shares generator spellings. Regenerate application
bindings to use the new lazy path; older bindings keep their checked fallback.

The full run passes 3,968 all-feature workspace tests with zero failures and
nine existing ignores, all 26 validation gates, strict core/transport linting,
Rust 1.85 native/Tower/owned/validated-lazy checks, 18 message-presence
comparisons and 41 mixed-peer tests. Default and regenerated owned bindings
each pass three pinned official conformance runs: 5,631 binary/JSON and 909
text cases, zero skips or failures, maximum Edition 2023. Seven of 13 default
bundled bindings change for the new decoder; the drift check passes.

The later transport source `6d1336e0` reuses bounded compressed output and
keeps the earlier identity-framing growth policy. It passes 3,980 all-feature
workspace tests (zero failures, nine existing ignores), strict transport and
adapter linting, mixed-peer tests, Rust 1.85 native/Tower builds, three default
conformance runs and regeneration checks. The earlier full core and isolated
owned records keep their `71422f00` source pin; those inputs are unchanged.

The goal of beating tonic/prost on every benchmark remains unmet. In the
earlier `71422f00` campaign, owned
parse/read medians beat Prost in 15 of 16 rows; all 16 encoding holdouts favor
pbrs. Lazy deep full reads, small and duplicate-heavy maps, small release
regeneration and several streaming costs still lose. Corrected generator timing
has six of six canonical debug rows and four of six matched release rows below
Prost. Consumer build costs are a separate unmeasured axis for this source.
The [benchmark guide](../benchmarks.md) keeps the measured scope and remaining costs.

Earlier [transport and naming additions](parity-and-maps-20261027/README.md)
provide native TCP_NODELAY, idle keepalive policy, connection executors, type
names, type-URL domains, retained enum prefixes and named-message `Any`
conversions. Those APIs remain available through the
[transport](../guides/transport-controls.md) and [codegen](../guides/codegen.md) guides.

The remaining migration gaps include `btree_map`, `bytes` and `boxed` codegen
controls, an alloc-only runtime, and custom executors in the static balanced
channel helpers. The [parity summary](../parity.md) and
[comparison](../guides/comparison.md) describe their scope.

The [server and nested-message follow-up](performance-fixes-20261027/README.md)
adds coalesced connection wakes and an explicit owned-message codegen profile.
Five local pairs show lower native-server CPU; owned storage avoids repeated
validation when applications read nested fields. The default lazy layout
preserves its partial-read and unchanged-forwarding behavior. See the
[benchmark guide](../benchmarks.md) for costs and the
[codegen guide](../guides/codegen.md#owned-nested-message-fields) to select a profile.

The earlier [direct parity audit](parity-proof-20261027/README.md) at
`e14cccb5` passes 3,921 all-feature workspace tests with nine existing ignores,
strict linting, Rust 1.85 native/Tower checks and official protobuf conformance.
All 18 raw-wire tonic comparisons and 41 mixed-peer tests pass after two
transport repairs. The audit also reproduces an unsupported alloc-only
deployment profile, protobuf behavior differences and deep read-all performance
losses. See the [short proof summary](../parity.md) for what passes and what fails.

The [October 27 record](grpc-parity-and-costs-20261027/README.md)
collects earlier combined checks at source `a24c0e35`: 3,902 all-feature
workspace tests passed, with nine existing ignores; official protobuf
conformance and five Rust 1.85 library profiles passed. The workspace run
began one test-only commit earlier, as the record explains. That revision
also has source-pinned RPC comparisons and retains its measured losses.
The subsequent [MiniTable group and unknown-field follow-up](minitable-groups-20261027/README.md)
passed targeted checks, Rust 1.85 profiles and official conformance at
`6fba2435`.
The [allocation and generator follow-up](efficiency-20261027/README.md)
at `31e7d69d` passed 202 targeted core tests, 750 gRPC library tests, strict
linting, Rust 1.85 profiles and official conformance. Its component performance
results include the remaining timing and memory losses.

These checks are not a production certification. Complete protobuf and gRPC
feature coverage, sustained-resource testing and broader performance
qualification remain open. Use the [work queue](../../TODO.md) for next steps,
the [comparison](../guides/comparison.md) for migration gaps, and
[benchmarks](../benchmarks.md) for measured costs. Earlier attempts and their
source limits are preserved in the [October 26 status history](status-history-20261026.md).
