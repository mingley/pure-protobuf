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
the same wire values. A fresh
default request must reset prefix retention and omit the optional `Name` impl;
setting a domain alone must not enable that impl.

Three small pbrs consumers still reproduce absent `btree_map`, `bytes` and
`boxed` build methods. These are API and representation customization gaps.
pbrs provides its own map and byte storage; an absent method does not establish
a wire-format failure. The Prost control generates and checks each requested
representation independently.

The alloc-only control builds a `#![no_std]` library using prost's serialization
API with default features off for `thumbv7em-none-eabi`. The equivalent pbrs
consumer also disables all default features. The control requires prost to
compile and pbrs to report a missing `std` crate before it records the deployment
gap. This distinguishes lack of target installation from lack of runtime
support. It checks compilation, not execution on embedded hardware.

The `pbrs_btree_map`, `pbrs_bytes` and `pbrs_boxed` binaries deliberately fail
compilation. Use `run.py`, which verifies those diagnostics and executes the
positive consumers, rather than `cargo test --all-targets` on this isolated
proof package. If a remaining capability is implemented, replace its expected
counterexample with a behavioral regression test.
