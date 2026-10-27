# Tonic/prost parity and performance proof

The audit found concrete gaps and performance losses. The follow-up fixes
server scheduling costs and adds an owned-message profile for applications
that read nested messages. Full tonic/prost parity and a world performance
ranking remain open. The [original audit](evidence/parity-proof-20261027/README.md)
and [performance fixes](evidence/performance-fixes-20261027/README.md) retain
failures, controls, source pins and all measured rows.

## Compatibility

| Check | Result |
|---|---|
| Protobuf parse and merge against prost 0.14.4 | 652 of 654 proto3 operations agree. The other two use the same wrong-wire-type input. |
| Missing proto2 required field | pbrs rejects it; prost accepts it. |
| Generated consumers | Nine same-schema roundtrips pass, including imports, oneof, presence, maps, bytes, Timestamp and external mappings. |
| Embedded, alloc-only runtime | Prost compiles for `thumbv7em-none-eabi`; pbrs requires `std` and fails. |
| gRPC message presence against tonic 0.14.6 | All 18 raw-wire comparisons pass after the repair. |
| Existing mixed-peer suite | All 41 tests pass after restoring native connection information in the tonic service adapter. |

A wrong wire type for a known field is preserved as unknown by pbrs and
rejected by prost. Missing required-field handling also differs. These are
measured policy differences, not evidence that pbrs should abandon protobuf
conformance to imitate prost.

Full functionality also needs custom executors, an independent idle keepalive
policy, and configurable TCP_NODELAY. Those tonic capabilities have no native
equivalent today. Six `prost-build` configuration probes reproduce API and
representation customization gaps; these alone do not establish wire failures.
HTTP/1.1 gRPC-Web is available through the documented Tower/Hyper recipe.

Both lazy and owned bindings pass the pinned protobuf 35.1 binary/JSON and
text conformance suite through maximum Edition 2023. The
[follow-up record](evidence/performance-fixes-20261027/README.md) retains the
isolated owned regeneration and the unchanged default-binding checks.

The [comparison guide](guides/comparison.md) covers the broader migration
surface. Passing a finite corpus establishes its tested behavior; it cannot
establish every possible schema, API or wire input.

## Performance

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
1,441 ns versus lazy's 809 ns. The mixed-message full read still loses to
Prost. These are five randomized process pairs per row, using separate CPU
and allocation builds; the complete record includes every fixture and repeat.

Encoding favors the default layout for these nested fixtures. At depth 64,
owned first encode costs 567 ns versus lazy's 26 ns and Prost's 6,831 ns;
owned cached encode costs 329 ns versus 30 ns and 6,750 ns. The owned profile
also allocates the full tree during a partial read. These tradeoffs are why
the default remains lazy.

The [codegen guide](guides/codegen.md#owned-nested-message-fields) shows the
builder and plugin options. Owned messages use a compact size cache; very
large sizes are recomputed rather than truncated. This does not impose a
message-size limit. Default generated output remains unchanged.

The [complete performance record](evidence/performance-fixes-20261027/README.md)
separates parsing, full and partial reads, encoding, requested allocation bytes
and endpoint CPU. It retains slower cases and rejected experiments. The
[original audit](evidence/parity-proof-20261027/README.md) explains why lazy
read-all traversal is expensive: a 64-node chain revalidates 2,016 descendant
nodes before materialization. The owned profile avoids that traversal; it does
not change the default lazy layout.

The compiler audit checks generated behavior, not generator or consumer build
speed. The [earlier codegen matrix](evidence/sb09-codegen-matrix.md) retains
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
