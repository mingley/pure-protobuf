# Adoption matrix contract (SB-26)

Base: `34fffc29dbf872f642b38baafb8e2ed90f518832`. Tier: dev-loop.

The matrix compares generated messages from the same public, synthetic
schemas. It measures every field that the schema declares, including string
and bytes contents, optional presence, selected oneof variants, repeated
elements, map keys/values, recursive queries and the decoded public payloads
inside `google.protobuf.Any`. Fixture construction and equivalence checks
happen before the timing and allocation window.

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

SB-26a lands and qualifies all five generated corpora and equality/read-all
oracles. SB-26b registers codec/RPC cells, executes the baseline, and records
P1-P5 measurements and limits. SB-26 stays open until both children and every
parent acceptance requirement have evidence.

The standalone corpus consumer reuses `pbrs`, prost/prost-build 0.14 and
serde_json, all already in the established benchmark graphs. Its manifest
is reviewed before resolution/building. It adds no shipping dependency,
alternate TLS provider, unsafe code or benchmark-specific runtime behavior.
