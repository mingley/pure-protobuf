# Code generator parity checks

This suite compares pbrs with prost-build and prost 0.14.4. It reports selected
representation controls and the legacy standard-library target boundary. It does
not certify full compiler equivalence or measure performance.

Run from the repository root, with Cargo and protoc on `PATH`:

```sh
rustup target add thumbv7em-none-eabi
python3 tests/proof/codegen_parity/run.py --output work/proof-codegen/run
```

The checked-in lockfiles support offline execution after their dependencies
are cached. `CARGO_TARGET_DIR` can point to an existing shared build directory.
The script retains commands, exit codes, standard output, standard error,
source hashes, compiler versions and the Git revision in the output directory.
Choose a new output directory for every run; the driver refuses to overwrite
earlier evidence.

The generated consumer compiles both generators' output from the same schema.
Nine cases cover imported nested types, an enum, oneof choices, optional scalar
presence including explicit zero, map values, arbitrary bytes, Timestamp and
an external type mapping. It decodes prost output with pbrs and decodes the
pbrs reserialization with prost, then checks value equality. Map wire ordering
is intentionally unconstrained.

The configured consumer compares `retain_enum_prefix`, `enable_type_names` and
`type_name_domain` against Prost. Eight name cases check original names,
packages, nested full names and type URLs, including exact, package, suffix,
duplicate and default-domain rules and a trailing slash. Four `Any::from_msg`
and `to_msg` roundtrips cover custom URLs, nested names, empty packages and a
built-in Timestamp. Twelve URL cases and a truncated payload compare success
and failure directly with Prost. Retained enum constants, proto2 explicit
defaults and aliases, packed values and proto3 unknown numeric values preserve
the same wire values. Two additional enum controls compile and use `Self_`
for the legal protobuf value `SELF`, with and without enum-prefix retention;
`FOO_SELF` becomes `Self_` by default and `FooSelf` when retaining its prefix.
Singular and repeated values roundtrip through both codecs. A fresh
default request must reset prefix retention and omit the optional `Name` impl;
setting a domain alone must not enable that impl.

Thirteen field-attribute selectors are also generated with both pbrs and
prost-build. The check compares selected protobuf fields for the global path,
fully qualified prefixes, relative suffixes and component-boundary nonmatches.
One message places its non-scalar fields in pbrs's cold storage. This checks
attribute placement, not the compatibility of all Rust attributes or layouts.

Three additional consumers compile schemas whose names collide with generated
`View` and `Mut` types or reuse the same nested parent and leaf names. Lazy and
owned consumers check imported singular, repeated and map values against Prost;
the third maps a renamed message type to a separately generated provider and
uses its nested type. The build also compares binary-only generated files from
combined, reversed and separate requests. Synthetic map-entry descriptors must
not reserve public names. These are selected naming controls; other cases,
such as two message names that produce the same module name, remain outside
their scope.

The lazy and owned runtime-symbol consumers keep the literal schema names
`Default`, `Map` and `Enum`. They exercise oneof resets, default typed views,
mutators and cross-file singular, repeated and map fields against Prost.
Generator-owned traits and map containers must resolve independently of those
schema names. Other runtime and prelude name collisions remain outside this
bounded repair.

Three boxed-field consumers run the same eight case groups with selected,
default and global-owned layouts. Selected singular messages use eager
`Option<Box<T>>` storage while ordinary accessors stay unchanged. The checks
cover shared byte ownership, imported children, duplicate-message merging,
oneof replacement, required-field policy, malformed input, recursion limits,
and JSON/text roundtrips against the pinned Prost wire control. Builder and
descriptor-set generation must produce identical source. Eighteen selector
cases compare message declarations, including real one-member oneofs,
synthetic optional fields, qualified prefixes, suffixes, boundaries and the
repeated/map exemption. The `*` spelling is literal, as in Prost. Compiled
reference configurations select eligible messages: Prost can emit invalid
scalar derives for unrestricted scalar boxing, so the placement matrix is
not a claim that such scalar configurations compile.

The [selected bytes controls](bytes.md) compare real Bytes-backed storage and
twenty-one selectors with Prost. Selected, owned and safe-default consumers
cover payload sharing, presence/defaults, mutation, JSON/text and segmented
output. Focused mode runs with `--only bytes`. Older-parent copying, duplicate
map removal and reflection-off complex codecs remain explicit controls and
counterexamples.

The [ordered-map consumer](../ordered_maps/README.md) checks real unique tree
storage, duplicate replacement, sorted iteration/encoding, parent paths,
mutation and codecs against pinned Prost controls. Run it alone with
`--only ordered`. Old lazy ancestors keep their original raw wire until
mutation; the consumer records this boundary and checks regenerated parents.

The legacy no-default target control builds generic serialization APIs for
`thumbv7em-none-eabi`. Prost compiles without default features; pbrs intentionally
retains its existing std profile when only default features are disabled, so
that control requires a missing `std` diagnostic. Target installation failures
do not count as that result. This records the legacy profile's boundary; it
does not test the explicit alloc-only runtime. The separate
[concrete alloc-only consumer](../alloc_runtime/README.md) builds real native
and Prost messages with std host codegen and an alloc-only target runtime.
Neither target compilation check executes firmware.

The `pbrs_bytes_lightweight_gap` binary deliberately fails compilation.
Use `run.py`, which verifies its diagnostics and runs the
positive consumers, rather than `cargo test --all-targets` on this isolated
proof package. Replace each expected counterexample when the corresponding
capability is implemented and verified.
