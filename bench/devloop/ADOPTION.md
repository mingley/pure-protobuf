# Adoption matrix contract (SB-26)

Base: `34fffc29dbf872f642b38baafb8e2ed90f518832`. Tier: dev-loop.

The matrix compares generated messages from the same public, synthetic
schemas. It measures every field that the schema declares, including string
and bytes contents, optional presence, selected oneof variants, repeated
elements, map keys/values, recursive queries and the decoded public payloads
inside `google.protobuf.Any`. Input preparation and equivalence checks happen
before the timing and allocation window. Fresh-encode construction happens
inside that window.

## Corpora

- Query: 48 deterministic variants, eight at each depth from three through
  eight, with Boolean operators, typed predicates, subqueries and edge
  traversals. Each encoded query is between 100 and 800 bytes.
- Entity lists: 10, 100 and 1,000 records, with keys, revisions, optional
  descriptions, nested edges and Any payloads of 256, 320, 384, 448 or 512
  encoded bytes. Any carries a generated synthetic `Payload`, not captured
  application data.
- Sparse: 128 optional fields spanning Boolean, signed integer, floating
  point, string, bytes and message values. Ten presence patterns set about
  one in ten fields, including explicitly present scalar defaults.
- Maps: 8, 64 and 512 entries per scalar/value map, plus message-valued maps.
  Read checksums consume every entry and are independent of iteration order.
- Codegen: twenty synthetic files importing a shared proto2 custom-options
  definition, which imports `google/protobuf/descriptor.proto`. Compare
  generated source and fresh/incremental consumer check costs on this same
  file set.

An independent generated prost representation is the full semantic equality
oracle. Decode pbrs's encoded output with prost and compare every field to
the original prost specimen; parse prost's output with pbrs and compare the
two independent complete read walks. Wire order, especially maps, need not
be identical. Retain byte lengths, checksums and source pins in JSON. Checksums
alone do not prove equality. Generated output is produced by build scripts;
only schemas and their reproducible fixture-generation script are committed.

## Workloads and accounting

Codec rows cover fresh encode (construct inside the window), decode without
field access, decode and read every field, and clone of a fully read message.
Cached encode remains a separately named diagnostic and cannot stand in for
fresh encode. Keep input ownership equivalent and document owned/shared-buffer
materialization. Every timed output is consumed.

RPC rows cover unary and server streaming across prost/pbrs messages and
tonic/pbrs-grpc transports. Every handler reads the entire request and every
client reads the entire response. Request/response bytes, counts and read
checksums must agree across the four profiles before measurement. Run peers
at their defaults; expose any workload-required message cap explicitly and
apply it equally. Do not compare the existing unequal-work legacy rows as an
adoption result.

Use the existing exact counting allocator and differential instruction
collector: identical preparation at N and 2N, `(count_2N - count_N) / N`.
Record every cell and the P1-P5 target status, with missing tools, unsupported
integration profiles and unavailable measurements visible. macOS allocation
and wall-time results cannot substitute for Linux instruction evidence.
Cold first-RPC and codegen/check timing are separate P4/P5 measurements.
Do not change the benchmark-contract thresholds or claim-grade requirements.

## Execution

SB-26a qualifies all five generated corpora and equality/read-all oracles.
SB-26b coordinates three bounded measurement cards: SB-26c registers and
measures codecs, SB-26d measures identical-work RPC profiles, and SB-26e
measures startup/codegen costs and scores P1-P5. SB-26 stays open until all
children and every parent acceptance requirement have evidence.

SB-26c registers each of the 64 runtime specimens separately for pbrs and
prost and all four workloads: 512 cells, without pooling variants. Native
fresh construction uses generated setters and containers directly; it does
not convert a prost message. Each codec builds and encodes its own nested Any
payload inside the fresh window. Decode starts from the same borrowed wire
slice and returns an owned message. Read-all consumes every declared field
and decoded Any payload. Clone starts from a fully read owned message and
consumes the cloned output without adding a field walk to the clone window.
The existing allocator and N/2N collector also count output destruction.
Preparation, validation and warmup are identical at N and 2N. Initial full
matrix runs may use an explicit small iteration count; retain that count and
each collector's measured or unavailable status without changing thresholds.
The initial full codec run is frozen at N=16, 2N=32 and three repeats, with
the collector's unchanged 100-operation warmup. It retains eight 64-cell
reports (one per codec/operation), whose union must match all 512 registered
IDs. Fixed-order dev-loop samples and wall time under instrumentation do not
satisfy claim-grade statistics or establish native wall-time speed.

The reviewed SB-26c manifest change is a local path dependency on the existing
`pbrs-adoption-corpus` library from the excluded devloop consumer. Its graph
already uses the benchmark's pbrs, prost/prost-build 0.14 and serde_json.
Review the lockfile delta before building; shipping manifests stay unchanged.

The standalone corpus consumer reuses `pbrs`, prost/prost-build 0.14 and
serde_json, all already in the established benchmark graphs. Its manifest
is reviewed before resolution/building. It adds no shipping dependency,
alternate TLS provider, unsafe code or benchmark-specific runtime behavior.

SB-26d registers all 64 specimens for each of `native_pbrs`, `native_prost`,
`tonic_pbrs` and `tonic_prost`, separately for unary and four-message server
streaming: 512 RPC IDs. Both templates are decoded from common prost wire
and fully read before measurement; each RPC clones its own template. Both
handlers consume every request field, then return the complete request once
or clone it into four complete responses. Each client consumes every field
of every response and verifies the count. A separate network call before
warmup verifies full decoded equality; preparation is identical at N and 2N.

These use typed native calls and tonic's low-level Grpc client/server with
the same message schemas. The tonic pbrs codec uses the existing adapter's
direct encoding/shared decode recipe; tonic prost encodes directly into
tonic's buffer and decodes using prost's Buf decoder. Native prost uses the
shipping native wrapper, including its existing temporary encoding buffer.
This measures both codec/transport layers without pretending TC-29/30's
unmodified tonic-codegen transport adapters exist.

Initial RPC measurements use N=16, 2N=32, three repeats, the unchanged
100-RPC warmup, and the same two-worker Tokio runtime for every profile.
No message caps are overridden: all qualified specimens fit both defaults.
Exact allocator and the parent's instruction/syscall collectors are reused.
Map-heavy messages retain their original default-valued entries. Their four
extra native bytes block all 24 map/profile/shape combinations until a
separate codec correction makes their wire work equal. The other 488 cells
can be measured, but this does not satisfy the complete SB-26d matrix.
The runner can collect independent reports concurrently via `--jobs` (one
by default, at most four), recording that count. N/2N and repeat order within
each cell stay sequential. The current shared cloud host has five available
CPUs; three collectors leave capacity for the host. Concurrent instrumented
wall time is secondary and must not be presented as native latency evidence.
The tonic/prost profile delegates encoding to the actual tonic-prost encoder;
it must not add an extra encoded-length pass or a buffering policy of its own.
