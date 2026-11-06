# Ordered generated maps

This consumer checks `Config::btree_map` against prost-build and Prost 0.14.4.
Selected fields contain real `pbrs::TreeMap` storage backed by
`std::collections::BTreeMap`. Parsing replaces duplicate keys, iteration follows
key order, and removal erases the key. Ordinary `Map` behavior stays unchanged.

Run from the repository root:

```sh
python3 tests/proof/codegen_parity/run.py --only ordered --output work/ordered-proof
```

The full codegen parity driver also runs this consumer. It needs `protoc` on
`PATH` or in `PROTOC`; Cargo dependencies must be available offline.

The three compiled profiles cover selected fields, owned message fields, and
`forbid(unsafe_code)` with safe defaults. Each runs eight groups:

- Unique sorted entries, duplicate replacement, and unchanged ordinary maps.
- Singular, repeated, map, oneof, nested, and transitive parent messages.
- `proto!`, view copy, message copy/merge, mutation, and size cache updates.
- Boolean, signed, unsigned, and string keys with scalar, enum, bytes, and message values.
- Child/parent JSON and text, and descriptor-based binary roundtrips.
- Selected byte retention, whole-tree view conversion, and segmented encoding.
- Final required-value checks, duplicate value merging, and groups.
- Recursion limits and malformed descendants.

JSON/text controls are pbrs roundtrips. Prost controls cover wire values and
storage selection.

Eighteen source controls compare exact selector placement and replacing/clearing
builder calls with prost-build. Descriptor-set and plugin-request entry points
must emit the same selected bindings. A following unconfigured request checks
state reset. Empty and unmatched selections must equal current default output;
its hashes must also match the frozen 05a3 generator in `baseline-default.json`.

## Wire and ownership boundaries

Newly generated singular paths to selected map descendants become eager, so
encoding reaches their ordered storage. The frozen 05a3 parent and grandparent
in `old_parent/` keep their original wire after reads. Explicit
`LazyMsg::{from_wire,from_validated_wire,from_parsed}` caches do too. Their typed
child views are ordered, but their cached bytes can retain duplicate keys and
unsorted entries until mutation. Regenerate the ancestors with the same
selection to make their writes reach the tree. Extern-path providers must also
generate their own selected storage.

Map entries keep pbrs's existing explicit encoding of default key/value fields;
Prost may omit those fields. Value equality and sorted entries do not imply
identical bytes. String query conversion may allocate. Copying an iterator of
borrowed byte slices copies payloads; `TreeMap::from(view)` clones the actual
tree and retains its `ProtoBytes` backing. `TreeMapMut::copy_from_view` does the
same for an existing map, including message values. The iterator form
`copy_from` requires owned or convertible values; generated message views do
not currently implement that conversion.

`baseline-parent-json.json` preserves the actual 05a3 runtime failure for a parent
containing a child with a map; text and binary roundtrips succeeded. The
manifest pins its source, generator and runtime hashes. Current parent JSON
controls require the canonical descriptor lookup fix in `src/json.rs` and check
all three selected profiles plus unchanged default generated source. The
historical failure is recorded separately from those current positives.

These finite controls do not prove exhaustive feature parity or a performance
ranking. They make no timing or allocation-free claim.
