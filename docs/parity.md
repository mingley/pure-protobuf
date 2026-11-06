# Tonic/prost parity and performance proof

Current source supports selected eager boxed fields, ordered maps and shared byte storage,
an opt-in generated forbid-unsafe policy, explicit alloc-only native messages,
and direct endpoint URI origins.
The consumer controls below establish those features' tested scope; remaining
gaps and measured costs stay open.

The [October 29 runtime and codegen record](evidence/runtime-and-codegen-20261029/README.md)
measures `0a770792`. Of 52 standard thin-LTO CPU rows, 49 paired medians favor
pbrs over Prost; all 16 standard owned parse/full-read medians favor pbrs. Some
lazy reads still lose, and several cases regress against prior pbrs. Strict
Prost comparisons retain known differences; full tonic/prost parity and an
overall ranking remain unproven.

The [October 27 decoder, map and generator record](evidence/decode-and-map-speed-20261027/README.md)
compares `71422f00` with `0287879d`. It removes repeated descendant validation
from regenerated lazy access and repeated index searches from warm unique-map
iteration. Its comparisons retain their original source scope.

The [original audit](evidence/parity-proof-20261027/README.md),
[server/owned-storage fixes](evidence/performance-fixes-20261027/README.md) and
[transport/naming follow-up](evidence/parity-and-maps-20261027/README.md) retain
their own source pins, controls, failures and full measured rows.

## Compatibility

| Check | Result |
|---|---|
| Protobuf parse and merge against prost 0.14.4 | 652 of 654 proto3 operations agree. The other two use the same wrong-wire-type input. |
| Missing proto2 required field | pbrs rejects it; prost accepts it. |
| Generated consumers | Base roundtrips and configured naming, type-URL, Any, enum and attribute controls pass against pinned Prost; see the [consumer guide](../tests/proof/codegen_parity/README.md) for exact cases. |
| Selective source comments | [15 compiled consumers](../tests/proof/comment_selectors/README.md) compare imported/nested/message/field/enum selectors and wire values with Prost 0.14.4. Builder/plugin/FDS, reset and parallel controls pass; three RPC modes have source-only comment checks. |
| Field view accessor names | [25 schema cases](../tests/proof/field_views/README.md) compile accessor APIs and exchange values with Prost 0.14.4 in lazy, owned, safe and owned+safe profiles; valid existing APIs keep their names. |
| RPC service names and aliases | [55 names](../tests/proof/rpc_names/README.md) compile in native, Tonic, compat and prost-native output: 220 consumers, 12 strict aliases, 61 native/adapter alias controls and a pinned Tonic reference. |
| Selected ordered maps | [24 consumer groups](../tests/proof/ordered_maps/README.md), 18 pinned Prost selector comparisons and four parent JSON checks cover real unique ordered storage. Older lazy ancestors retain their documented raw-wire behavior. |
| Nested map JSON | [Resolved-descriptor tests](../tests/json_nested_descriptors.rs) cover recursive/imported maps, all parent containers, required validation and descriptor fallback. |
| Selected boxed fields | Selected, default and global-owned consumers each pass eight groups; 18 message-field selector placement controls compare with Prost 0.14.4. |
| Selected bytes storage | [Consumer controls](../tests/proof/codegen_parity/bytes.md) compare selectors, values and shared/borrowed input with Prost 0.14.4 in selected, owned and forbid-unsafe profiles. Old eager-parent copying and format-generation limits remain explicit. |
| Generated forbid-unsafe policy | Twelve checks pass across lazy, owned and boxed consumers; the ordinary generated profile fails the intentional forbid-unsafe control. Runtime dependencies keep their own unsafe implementations. |
| Endpoint URI origins | [Eight public tests](../pbrs-grpc/tests/endpoint_origin.rs) cover direct native and Tonic URI routing, all four call shapes, TLS/SNI and clone overrides. Static balance helpers reject per-endpoint origins. |
| Balanced endpoint authority | [23 public tests](../pbrs-grpc/tests/balance_executors.rs) cover selected URI authority, all four native and opaque Tonic call shapes, clone overrides, retries, hedges, health, binary logs and endpoint replacement. Per-endpoint origins, TLS and hostname lookup remain unsupported. |
| Embedded, alloc-only runtime | The [concrete consumer](../tests/proof/alloc_runtime/README.md) builds five native profiles and Prost 0.14.4 messages for `thumbv7em-none-eabi`, with separate std host codegen. Host functional checks explicitly enable std; firmware execution remains unverified. |
| gRPC message presence against tonic 0.14.6 | All 18 raw-wire comparisons pass after the repair. |
| Existing mixed-peer suite | All 41 tests pass after restoring native connection information in the tonic service adapter. |

A wrong wire type for a known field is preserved as unknown by pbrs and
rejected by prost. Missing required-field handling also differs. These are
measured policy differences, not evidence that pbrs should abandon protobuf
conformance to imitate prost.

Default `Map` removal preserves unrelated entries' relative order and duplicate-key
winners. It still removes only the target key's last stored occurrence;
an earlier parsed duplicate can reappear, unlike Prost/HashMap deletion.
The [removal record](evidence/map-removal-20261106/README.md) includes all three
backends and measured small-map costs alongside indexed update gains.
Selected `TreeMap` fields store one value per key and remove that key completely;
see the [ordered-map guide](guides/maps.md) for generation and wire boundaries.

The native transport now supports configurable TCP_NODELAY, client idle
keepalive policy and custom connection-driver executors. The
[transport guide](guides/transport-controls.md) explains defaults and scope.
Executor selection does not replace Tokio I/O, timers or helper tasks. Static
and watched balance helpers preserve per-URI executors, including URI aliases
of one socket, and drain replaced connections. Their update API remains a
full snapshot. Each RPC attempt uses its acquired connection's endpoint
authority unless `Channel::origin` explicitly pins the clone's authority.
Background health Watches use their own connection's endpoint authority;
channel getters and client call labels retain the configured channel context.
`Endpoint::origin(Uri)` sets logical request scheme and authority while
preserving dialing, TLS and interceptor security context. Its path and query
are ignored. `Channel::request_scheme()` exposes the logical scheme; a later
`Channel::origin(authority)` changes only that clone's authority.

Code generation supports `enable_type_names`, `type_name_domain` and
`retain_enum_prefix`, with configured behavior checked against Prost. Shared
well-known `Any` messages support typed `from_msg` and `to_msg` conversion.
Attribute selectors support global, qualified-prefix and relative-suffix
matching, including fields placed in cold storage. A generated-code control
compares 13 field selectors with Prost 0.14.4; see the
[codegen guide](guides/codegen.md) for the pbrs storage limits.
Enum values named `SELF` use Prost's legal `Self_` spelling. Type naming
reserves generated view and mutator names, rechecks repeated parent prefixes,
and shares nested aliases with imported paths. Lazy, owned and explicitly
mapped external consumers pass the focused collision controls; the
[naming guide](guides/codegen.md#generated-names-and-regeneration) explains
regeneration and the remaining module-name limits.
Generator-owned types, traits and value constructors use qualified paths. The
[namespace consumer](../tests/proof/namespace_symbols/README.md) checks 34
message names and enum names `Some`, `None`, `Ok` and `Err` in four
storage/default profiles with four runtime paths. It covers lowercase
`pbrs`/`std`/`core` shadows while preserving explicit user paths and
attribute/documentation literals. A separate
[RPC consumer](../tests/proof/rpc_names/README.md) checks framework and generic
name collisions in all four stub modes without renaming service identifiers.

`Config::boxed(path)` constructs selected singular children during parsing
while preserving pbrs accessors. Repeated and map fields retain their storage;
imported child types retain their generated profile. The
[selected-field guide](guides/codegen.md#box-selected-nested-fields) explains
selectors and the forwarding tradeoff. `safe_message_defaults(true)` lets
generated bindings compile under `#![forbid(unsafe_code)]`; it remains
opt-in and does not remove unsafe code from runtime dependencies. The
[consumer proof](../tests/proof/safe_defaults/README.md) covers its behavior.

[`Config::bytes(paths)`](guides/bytes.md) selects eager `ProtoBytes` storage
and adds shared getters. Borrowed singular getters stay `&[u8]`; shared getters
return owned handles. The guide explains input retention and the limits of
regenerating only child messages.

[`Config::btree_map(paths)`](guides/maps.md) selects ordered storage and makes
newly generated singular ancestor paths eager. Separately generated or explicit
lazy wrappers can still forward their original wire until mutation; their
typed child views do not rewrite that cache.

The [alloc-only guide](guides/alloc-only.md) covers the explicit runtime and
generation options. Legacy no-default builds retain std; enabling `std` wins
over `alloc-only`. Reflection, formats, transports and the `rust_out` ABI remain
std-only. HTTP/1.1 gRPC-Web is available through the documented Tower/Hyper recipe.

At `bc8cec38`, pinned protobuf 35.1 conformance passed for clean default
bindings and a fresh owned-binding overlay, through Edition 2023. The
[October 29 record](evidence/runtime-and-codegen-20261029/README.md) explains
the two source scopes.

At `71422f00`, both lazy and owned bindings passed the pinned protobuf 35.1
binary/JSON and text conformance suite through maximum Edition 2023. The
[October 27 record](evidence/decode-and-map-speed-20261027/README.md) retains fresh
isolated owned regeneration at `71422f00`. Its full workspace run passes 3,968
tests with nine existing ignores. Seven of 13 default bindings change for the
new lazy decoder; the remaining six are unchanged and the drift check passes.

Final transport source `6d1336e0` passes 3,980 all-feature workspace tests
with zero failures and nine existing ignores, plus fresh mixed-peer, native
and Tower Rust 1.85, default conformance and regeneration checks. The isolated
owned-conformance source remains `71422f00`; the core source is unchanged.

The [comparison guide](guides/comparison.md) covers the broader migration
surface. Passing a finite corpus establishes its tested behavior; it cannot
establish every possible schema, API or wire input.

## Performance

At `71422f00`, lazy depth-64 parse/read/drop falls from 28,300 to 3,968 ns in
this comparison, but still costs 2.339 times Prost's paired CPU. Mixed-message
full reads have paired pbrs/Prost ratios of 0.909 for lazy and 0.991 for owned
storage. Owned parse/read wins 15 of 16 median rows, with depth-64 parsing near
parity at 1.004. All 16 encoding holdouts favor pbrs.

Warm unique-map iteration at 256 entries falls from 1,943 to 94 ns, versus
253 ns for standard HashMap. Only the full unique-map rows at 16, 64 and 256
entries beat that control; small and duplicate-heavy maps remain losses.
The corrected generator timer shows six of six canonical debug rows and four
of six release rows below Prost. Small release generation remains slower,
output remains much larger, and downstream build costs are unmeasured for this
source. Streaming scheduling and some transport allocation costs also remain
open. See [benchmarks](benchmarks.md#latest-runtime-and-generator-results-october-27)
for the October 27 tables and [raw evidence](evidence/decode-and-map-speed-20261027/README.md)
for every row and rejected prototype.

The earlier map and nested-message results below describe their pinned sources.

The earlier map-only comparison against `77f361eb` improves all 42 warm typed
iteration rows. Collection uses 11.1–44.2% less CPU. Full mixed-message reads
remove one allocation and 128 requested bytes in both storage profiles. Lazy
read CPU improves 3.62%; owned read CPU regresses 5.85%, and owned parsing
regresses 16.3%. Sparse reads regress about 20%. Large-map iteration also
remains slower than the standard HashMap representation used by Prost. See
[benchmarks](benchmarks.md#typed-map-iteration-october-27) for the table and
measurement scope. Allocation reductions do not establish a CPU improvement.

The earlier server and nested-message results below describe their pinned
source in the performance-fixes record.

The server now coalesces wakes that arrive while its connection task is
running. A bounded number of local polls can make progress before rescheduling;
Tokio's cooperative budget still limits work. This removes redundant scheduler
round trips without changing codecs or handlers.

| Same-Prost-codec work | Native server before | Native server after | Median paired CPU change |
|---|---:|---:|---:|
| 1 KiB unary RPC | 45 µs/RPC | 23.5 µs/RPC | −54.8% |
| 64 × 1 KiB pipeline RPC | 270 µs/RPC | 200 µs/RPC | −28.0% |

Five before/after pairs complete 1,890,000 RPCs with no terminal errors. Fresh
tonic controls have a unary server median of 32.5 µs/RPC; the paired
native/tonic ratio is 0.723, with one pair above one. The client before/after
result is inconclusive. Paired changes differ from ratios of displayed medians.
These shared-host measurements establish a local improvement, not a ranking.

For nested messages, choose storage when generating code:

- The default lazy profile validates child wire during parsing and constructs
  child messages on access. It is useful for partial reads and forwarding
  messages unchanged, including cached encoding.
- `Config::owned_message_fields(true)` constructs singular children during
  parsing. Reading the resulting tree avoids repeated descendant validation.
  Use it when applications read or mutate most of a nested tree. Imported
  message types keep the profile with which they were generated.

| Parse, read and drop | Default lazy CPU | Owned CPU | Prost CPU | Paired owned/prost |
|---|---:|---:|---:|---:|
| 8-level message | 660 ns | 130 ns | 144 ns | 0.902 |
| 64-level message | 29,925 ns | 1,523 ns | 1,594 ns | 0.963 |
| Mixed RPC-shaped message | 908 ns | 906 ns | 860 ns | 1.053 |

The deep-64 owned read is about 19.6 times faster than the lazy read in
this comparison. It requests 1,512 allocation bytes versus lazy's 5,344 and
Prost's 1,008. Parsing alone does more work up front: deep-64 owned CPU is
1,441 ns versus lazy's 809 ns. The mixed-message full read loses to
Prost at that source pin. These are five randomized process pairs per row, using separate CPU
and allocation builds; the complete record includes every fixture and repeat.

Encoding favors the default layout for these nested fixtures. At depth 64,
owned first encode costs 567 ns versus lazy's 26 ns and Prost's 6,831 ns;
owned cached encode costs 329 ns versus 30 ns and 6,750 ns. The owned profile
also allocates the full tree during a partial read. These tradeoffs are why
the default remains lazy.

The [codegen guide](guides/codegen.md#owned-nested-message-fields) shows the
builder and plugin options. Owned messages use a compact size cache; very
large sizes are recomputed rather than truncated. This does not impose a
message-size limit. The original owned-profile addition preserved default
generated output; the later lazy-decoder path requires regeneration.

The [complete performance record](evidence/performance-fixes-20261027/README.md)
separates parsing, full and partial reads, encoding, requested allocation bytes
and endpoint CPU. It retains slower cases and rejected experiments. The
[original audit](evidence/parity-proof-20261027/README.md) explains why lazy
read-all traversal is expensive: a 64-node chain revalidates 2,016 descendant
nodes before materialization. The owned profile avoids that traversal; it does
not change the default lazy layout.

The compatibility audit checks generated behavior. The latest corrected
generator comparison measures CLI elapsed time, while consumer build speed
remains unmeasured for that source. The [earlier codegen matrix](evidence/sb09-codegen-matrix.md) retains
build-cost losses; it does not establish a ranking for the current generator.
A follow-up output-buffer experiment reduced peak RSS but increased CPU in all
six measured generator cases, so it was rejected.

These measurements ran on one shared x86_64 VM. A scoped leadership claim
needs matched competitors, workloads, resources, successful calls and repeated
measurements on controlled hosts. The [benchmark contract](benchmark-contract.md)
defines those requirements. Comparing against tonic and prost alone cannot
establish leadership over other implementations.

## Reproduce and check claims

The [test index](../tests/proof/README.md) links the pinned consumers and commands.
The default protobuf differential command fails because the strict behavior
claim has counterexamples. Its explicit known-differences mode passes only
when exactly those counterexamples recur and the remaining cases agree.
The [parity audit workflow](../.github/workflows/parity.yml) runs that regression
check; green means the audit reproduced its results, including the gaps.

The [RPC CPU preflight](../tests/proof/ranking/README.md) checks raw comparative
campaigns and recomputes intervals. It rejects omitted required peers,
insufficient samples, changed sources/settings and incomplete calls. Its 13
tests include misleading winner labels, measured losses and synthetic data.
A numerical preflight cannot certify host isolation or honest measurements;
independent campaign review remains necessary.
