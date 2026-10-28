# Tonic/prost parity and performance proof

The latest [decoder, map and generator record](evidence/decode-and-map-speed-20261027/README.md)
compares `71422f00` with `0287879d`. It removes repeated descendant validation
from regenerated lazy access and repeated index searches from warm unique-map
iteration. It also retains remaining losses: the current comparisons do not
establish a win on every benchmark or complete tonic/prost parity.

The [original audit](evidence/parity-proof-20261027/README.md),
[server/owned-storage fixes](evidence/performance-fixes-20261027/README.md) and
[transport/naming follow-up](evidence/parity-and-maps-20261027/README.md) retain
their own source pins, controls, failures and full measured rows.

## Compatibility

| Check | Result |
|---|---|
| Protobuf parse and merge against prost 0.14.4 | 652 of 654 proto3 operations agree. The other two use the same wrong-wire-type input. |
| Missing proto2 required field | pbrs rejects it; prost accepts it. |
| Generated consumers | Nine original roundtrips plus 27 naming, type-URL, Any and enum controls pass against pinned Prost. |
| Embedded, alloc-only runtime | Prost compiles for `thumbv7em-none-eabi`; pbrs requires `std` and fails. |
| gRPC message presence against tonic 0.14.6 | All 18 raw-wire comparisons pass after the repair. |
| Existing mixed-peer suite | All 41 tests pass after restoring native connection information in the tonic service adapter. |

A wrong wire type for a known field is preserved as unknown by pbrs and
rejected by prost. Missing required-field handling also differs. These are
measured policy differences, not evidence that pbrs should abandon protobuf
conformance to imitate prost.

The native transport now supports configurable TCP_NODELAY, client idle
keepalive policy and custom connection-driver executors. The
[transport guide](guides/transport-controls.md) explains defaults and scope.
Executor selection does not replace Tokio I/O, timers or helper tasks; static
balance helpers reject per-endpoint executors.

Code generation supports `enable_type_names`, `type_name_domain` and
`retain_enum_prefix`, with configured behavior checked against Prost. Shared
well-known `Any` messages support typed `from_msg` and `to_msg` conversion.
Three configuration probes still reproduce representation/customization gaps:
`btree_map`, `bytes` and `boxed`. These are distinct from wire-format failures.
The alloc-only runtime also remains unsupported. HTTP/1.1 gRPC-Web is available
through the documented Tower/Hyper recipe.

Both lazy and owned bindings pass the pinned protobuf 35.1 binary/JSON and
text conformance suite through maximum Edition 2023. The
[latest record](evidence/decode-and-map-speed-20261027/README.md) retains fresh
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
for the latest tables and [raw evidence](evidence/decode-and-map-speed-20261027/README.md)
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
