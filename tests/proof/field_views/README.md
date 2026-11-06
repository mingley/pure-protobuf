# Field view accessor collisions

This consumer checks generated Rust methods for singular message and group
fields. A field named `child_view` must not collide with `child`'s generated
view method. The same rule covers raw keywords, renamed bases, and view names
that collide with presence, setter, or clear methods.

Run from the repository root with `protoc` on `PATH`:

```sh
cargo fetch --manifest-path tests/proof/field_views/Cargo.toml --locked
cargo run --manifest-path tests/proof/field_views/Cargo.toml --locked --offline
cargo run --manifest-path tests/proof/field_views/Cargo.toml --locked --offline --features owned
cargo run --manifest-path tests/proof/field_views/Cargo.toml --locked --offline --features safe
cargo run --manifest-path tests/proof/field_views/Cargo.toml --locked --offline --features owned,safe
```

The 25 schema cases compile the public accessor names and exchange messages
with Prost 0.14.4. They include repeated, map, and scalar fields that have no
view method; those keep their existing names. Suffix controls check the actual
methods of a renamed field, including helpers that only appear after a rename.
Imported messages named `ChildView` and `ChildMut`, an explicit external type,
real oneof transitions, and proto2 groups also compile and exchange values with Prost.

The `safe` profiles forbid unsafe code in the consumer and generated bindings.
These checks cover generated APIs and wire behavior. They do not measure
runtime or generation performance.
