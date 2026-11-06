# Ordered maps

Use `Config::btree_map(paths)` for unique map storage with keys in order.
This option is part of the unreleased `pbrs` 0.2.1 candidate.

For `proto/inventory.proto`:

```proto
syntax = "proto3";
package example;
message Inventory { map<string, int32> stock = 1; }
```

Select the field in `build.rs`:

```rust
pbrs::codegen::Config::new()
    .btree_map([".example.Inventory.stock"])
    .compile_protos(&["proto/inventory.proto"], &["proto"])?;
```

After including the generated `mod.rs`, use the usual pbrs accessors:

```rust
let mut inventory = example::Inventory::new();
inventory.stock_mut().insert("pens", 4);
assert_eq!(inventory.stock().get("pens"), Some(4));
assert!(inventory.stock_mut().remove("pens"));
```

`stock()` returns `TreeMapView`, `stock_mut()` returns `TreeMapMut`, and
`set_stock()` accepts `TreeMap`. Storage contains a real
`std::collections::BTreeMap`. Iteration follows key order. Later decoded entries
replace earlier values with the same key; removal erases that key completely.
Ordinary, unselected `Map` fields retain their existing behavior.

## Select fields

Selectors use protobuf names and match whole name components. A leading dot
selects a package, message or field prefix; other paths select suffixes:

| Selector | Selects |
|---|---|
| `.` | Every map field |
| `.example.Inventory` | Maps in that message and its nested messages |
| `Inventory.stock` or `stock` | Matching suffixes across packages |

Match the map field itself, such as `.example.Inventory.stock`, rather than
its synthetic entry or `value` field. `*` is literal. Each `btree_map()` call
replaces the previous selection; an empty iterator clears it. The plugin
option is `--pbrs_opt=btree_map=.example.Inventory.stock`.

## Parents and cached wire

Regenerate ancestor bindings with matching selectors. Newly generated singular
paths to selected map descendants become eager: parsing constructs those
children, and encoding reaches their ordered storage. Repeated children and
map values already use owned storage.

Older or differently configured lazy parents can still forward original,
unsorted or duplicate entries. Reading a child does not clear that wire;
mutations clear only the caches they touch. An ordered child view does not
guarantee normalized parent output. Explicit `LazyMsg` caches, including
`from_parsed`, keep their established raw wire behavior too. An `extern_path`
provider must generate its own selected storage; caller configuration cannot
change its fields or cached originals.

## Costs and encoding

pbrs writes explicit default key/value fields inside map entries. Prost may
omit them, so equal decoded maps can have different valid bytes.

String queries and view conversions may allocate. `TreeMap::from(view)` and
`TreeMapMut::copy_from_view` clone the actual tree, retaining `ProtoBytes`
owners for [selected byte values](bytes.md). Iterator `copy_from` requires owned
or convertible values; borrowed byte slices copy their payloads. Compare
parsing, lookup, mutation and encoding with representative inputs before
choosing a profile.

The [consumer proof](../../tests/proof/ordered_maps/README.md) records selectors,
duplicate winners, parent boundaries, mutation, JSON/text and shared-byte
controls. These functional checks establish no comparative performance ranking.
