# Code generator parity checks

This suite compares pbrs with prost-build and prost 0.14.4. It reports a
functional deployment gap and build configuration migration gaps. It does
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

The generated consumer compiles both generators' output from the same schema.
Nine cases cover imported nested types, an enum, oneof choices, optional scalar
presence including explicit zero, map values, arbitrary bytes, Timestamp and
an external type mapping. It decodes prost output with pbrs and decodes the
pbrs reserialization with prost, then checks value equality. Map wire ordering
is intentionally unconstrained.

The reference build also generates and checks output with `btree_map`, `bytes`,
`boxed`, `retain_enum_prefix`, `enable_type_names` and `type_name_domain`.
Six small pbrs consumers reproduce the corresponding absent build methods.
These are API and representation customization gaps. pbrs provides its own map,
byte storage, enum and descriptor APIs; an absent method does not establish a
wire-format failure.

The alloc-only control builds a `#![no_std]` library using prost's serialization
API with default features off for `thumbv7em-none-eabi`. The equivalent pbrs
consumer also disables all default features. The control requires prost to
compile and pbrs to report a missing `std` crate before it records the deployment
gap. This distinguishes lack of target installation from lack of runtime
support. It checks compilation, not execution on embedded hardware.

The `pbrs_*` binaries deliberately fail compilation. Use `run.py`, which expects
and verifies their diagnostics, rather than `cargo test --all-targets` on this
isolated proof package. If a capability is implemented, update its expected
counterexample into a positive regression test.
