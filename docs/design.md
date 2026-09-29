# Protobuf runtime design

`pbrs` generates ordinary Rust message structs. Parsing validates wire data
up front and delays some field allocation until an accessor needs it.
Encoding reuses a cached size and can send large `bytes` fields as shared
segments through the native gRPC transport.

There are two message implementations in this repository. This page first
describes the `protoc-gen-pbrs` path used by native services and the Tonic
adapter. The separate Google `--rust_out` compatibility runtime is described
at the end.

## Storage and defaults

Empty repeated fields and maps use nullable storage, avoiding an allocation
for each absent collection. Wide messages put selected cold fields behind an
`Option<Box<MsgCold>>`; frequently exercised fields stay on the main struct.
That split affects both empty-message size and parse allocations, so changes
need measurements across several message shapes.

Generated defaults use a zero-valid layout. Explicit bool presence uses
`OptBool`, whose zero value means unset; Rust's `Option<bool>` does not have
that representation. The proof obligations for every field type are in
[unsafe invariants](unsafe-invariants.md).

`src/testdata.rs` also contains handwritten benchmark types. For example,
its `Person` keeps up to four tags or scores inline. Results for those types
must stay separate from results for generator output.

## Parsing and buffer ownership

`Parse::parse(&[u8])` accepts borrowed input. Fields that outlive that call
need owned storage, sometimes shared through a private `Arc<[u8]>`.
`Parse::parse_bytes(Bytes)` lets current generated messages share the
caller's reference-counted buffer instead. These APIs produce owned message
objects; `FooView` borrows such an object and is not a view over raw wire bytes.

| Field shape | Storage and work |
|---|---|
| Scalars | Decode inline; scalar-only messages need no wire backing allocation. |
| Strings up to 23 bytes | Copy into inline `ProtoString` storage. |
| Longer strings | Choose an exact field copy or a wire window according to field/frame size. Near-whole fields and sparse fields in large frames copy exactly to limit retention. |
| `bytes` | Fields of at least 4 KiB can share a `parse_bytes` buffer. Smaller fields copy into their own storage to avoid retaining a large frame. |
| Packed varints | Validate the payload now; materialize the collection on access. Encoding produces canonical varints. |
| Packed fixed-width scalars | Keep a payload-only wire buffer; materialize the collection on access. |
| Nested messages | Validate the nested wire data; `LazyMsg` materializes the nested struct on access. |

Parse rejects malformed lengths, varints, invalid UTF-8 where required, and
excess nesting before returning success. Repeated scalar runs can reserve and
append values without dispatching through the full tag match for every value.
See [zero-copy paths](zero-copy.md) and [retained-memory budgets](resource-budgets.md)
for the transport and lifetime consequences.

Generated messages currently use inline `merge_inner` dispatch. The safe
table engine in `src/table.rs` also exists and backs `DynamicMessage` parsing.
Emitting tables for generated messages is the next PK-07 step, governed by
the [table parser decision](decisions/table-driven-parse.md). The existence
of the engine does not mean generated services already use it.

## Encoding and maps

`CachedSize` stores the encoded size in an `AtomicU64` and does not participate
in equality. Setters, mutable accessors, clear, and merge invalidate it.
The next size or serialization operation recomputes it.

Maps retain wire-order pairs with last-key-wins semantics. Small maps scan
their entries. At 16 entries, indexed reads can build one sorted index of
unique keys and last positions, then use binary search. Insert updates an
existing index; other mutations invalidate it. This index is distinct from
the compatibility runtime's raw map representation.

Ordinary encoding writes to `WireOut`. Native, uncompressed gRPC can carry
shared `bytes` fields of at least 32 KiB in separate segments. The
[zero-copy guide](zero-copy.md) gives the conditions and copy accounting.

## API and compatibility boundaries

Generated accessors follow the Google Rust application model: nested getters
return a view of the value or a default instance, presence uses `has_` and
`*_opt`, open enums retain unknown integer values, and `proto!` supports
construction and spread syntax. The exact tested surface and exclusions are
in [codegen compatibility](codegen-compatibility.md).

Google `protoc --rust_out kernel=upb` output uses `src/runtime/` instead of
the field-wise structs above. That experimental path supplies a pure-Rust
MiniTable/Arena stand-in behind the expected generated-code interface. It
still has semantic gaps, including enum conversion, generic unknown-field
retention, equality, debug output, and extensions. Passing the plugin's
conformance suite does not qualify this second runtime. The
[upb comparison](upb.md) and [kernel design](decisions/upb-kernel.md) explain
the remaining work.
