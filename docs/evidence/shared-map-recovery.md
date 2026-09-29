# QG-05: original shared enum-collection recovery

Date: 2026-09-29. Host: macOS `aarch64-apple-darwin`.

This record describes `c89608bd`, not later revisions. The separate
[closed-enum decoding follow-up](closed-enum-recovery.md) records the storage
and unknown-wire repair at `3a7aa128`, with a new exact-source qualification.

## Reproduction and scope

At base `deea5e3f91f9271dd7c2926375369c1e367d1287`, the original
`accessors_map_test::test_map_int32_enum` failed: `MapView { len: 1 }` yielded
zero elements after inserting `MapEnum::Foo`. The command was:

```sh
export PATH=/opt/homebrew/bin:$PATH
export CARGO_BUILD_JOBS=2
export PROTOC=/Users/mingley/dev/pure-protobuf/target/conformance-build/protoc-35.1.0
cargo test --locked --manifest-path rust_out_shared/Cargo.toml \
  --test accessors_map_test test_map_int32_enum -- --exact --nocapture
```

The existing pinned compiler reports `libprotoc 35.1`; its source is exactly
`35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03`, matching `vendor/google/SHA`.
The compiler and source were reused read-only from the main checkout; this
worktree's ignored `third_party/protobuf` symlink points at that pinned source.
On another machine, use `scripts/build-pinned-protoc.sh` and its printed binary.

Restoring maps exposed two previously hidden failures in the third original
crate: `test_repeated_enum_accessors` and `test_repeated_enum_set`. Both used
the same missing enum-to-view conversion. The fix therefore covers both map
and repeated enum collections. No original consumer assertions, generator
output, inventory, or exclusions changed.

## Conversion contract

Raw collection constructors dispatch on the original generator's entity tag.
Only `EnumTag` can construct the private enum codec, requiring
`__internal::Enum + TryFrom<i32>` and
`for<'a> Proxied<View<'a> = Self>`. Insertions use typed `Into<i32>`; reads use
typed `TryFrom<i32>`. Open enums retain unknown values and closed enums reject
unknown values before creating a Rust value.

The codec's same-type copy is justified by its private constructor's exact
associated-type equality and `Copy` bound, not layout or size. It captures no
references, preserves the collection's arena lifetime, and travels through all
view and mutable reborrow conversions. The former size-based map and array
insertion heuristics are removed. Raw mutable constructors now explicitly
require `unsafe`, with typed storage, ownership, lifetime, and exclusivity
contracts. Native collection value bounds remain unchanged.

The added original-generator library regressions cover enum insertion, lookup,
iteration, replacement, removal, clearing, setters, view/mutable reborrows,
serialization, and unknown values including negative numbers and integer
extremes. Closed-enum kernel fixtures use a real Rust enum with invalid
discriminants to ensure both map and repeated conversions reject unknown
numbers without constructing an invalid value under Miri.

## Qualification

Committed source `c89608bd2073f2fa7e7d30d7299b1fd08466904c` passed all 19
original crates (233 tests), plus three additional generated-API regressions,
in ordinary execution and under Miri. The exact-source rerun used a clean
detached worktree at `/Users/mingley/dev/pure-protobuf-qg05-qualification-20260929`.
Core tests passed 93 library, 38 native shared, 26 runtime, and 18 kernel tests.
The exact-source Miri rerun also passed all 93 library, 26 runtime, and 18 kernel
tests with strict provenance and leak checking enabled. Warning-strict Clippy
and rustdoc passed. Broader preliminary Miri exposed two
preexisting test-only scratch MiniTables that were never freed; the kernel
fixtures now reclaim their owned tables after the assertions without disabling
leak checking or changing runtime MiniTable ownership.

The retained [summary JSON](shared-map-recovery/summary-c89608bd.json) records
the source SHA, clean-worktree checks before and after execution, exact commands
and environment, toolchain/compiler identity, per-suite results, and SHA-256
hashes for all seven retained transcripts. The
[normal shared log](shared-map-recovery/shared-c89608bd.txt),
[Miri shared log](shared-map-recovery/miri-shared-c89608bd.txt), and
[Miri core log](shared-map-recovery/miri-core-c89608bd.txt) retain the complete
test results. These committed-source results supersede the preliminary runs
from the dirty integration worktree.

The exact same SHA also passed the Linux
[compatibility run](https://github.com/mingley/pure-protobuf/actions/runs/36615248310):
the `shared-consumers` job reports 19/19 crates, 233 passed, zero failed.
Its upstream-drift, upstream-cases, and gRFC-drift jobs passed; fuzz-campaign,
target-matrix, and miri-sanitizers were skipped and provide no coverage claim.

Toolchains: `rustc 1.98.1 (48a229cea 2026-09-01)`,
`cargo 1.98.1 (797e8a9bc 2026-08-05)`,
`cargo 1.100.0-nightly (e8cb624d5 2026-08-22)`, and
`miri 0.1.0 (e7769602ac 2026-08-24)`.

Commands, after the exports above:

```sh
bash scripts/test-rust-out-shared.sh
cargo test --locked -p pbrs --lib --test runtime --test upb_kernel --test google_shared
cargo clippy --locked -p pbrs --lib --test upb_kernel -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --locked -p pbrs --no-deps
export MIRIFLAGS='-Zmiri-disable-isolation -Zmiri-strict-provenance'
export CARGO_TARGET_DIR="$PWD/target/qg05-miri"
cargo +nightly miri test --locked --offline --manifest-path rust_out_shared/Cargo.toml -- --test-threads=1
cargo +nightly miri test --locked --offline -p pbrs --lib --test runtime --test upb_kernel -- --test-threads=1
```

Original consumer counts, identical in ordinary execution and Miri:

| Crate | Passed |
|---|---:|
| accessors_map_test | 35 |
| accessors_proto3_test | 16 |
| accessors_repeated_test | 29 |
| accessors_test | 50 |
| bad_names_test | 5 |
| child_parent_test | 3 |
| edition2023_test | 3 |
| enum_test | 17 |
| fields_with_imported_types_test | 3 |
| import_public_test | 1 |
| message_copy_merge_test | 8 |
| message_generics_test | 7 |
| nested_types_test | 2 |
| package_test | 2 |
| proto_macro_test | 9 |
| serialization_test | 30 |
| simple_nested_test | 8 |
| threading_test | 2 |
| utf8_test | 3 |
| **Total: 19 crates** | **233** |

This includes local macOS qualification and Linux shared-consumer CI; it makes no Linux sanitizer, 32-bit,
big-endian, or sustained fuzzing claim.
