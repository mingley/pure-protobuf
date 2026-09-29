# QG-05: closed-enum decoding recovery

Date: 2026-09-29. Qualified source:
`3a7aa12857e8d9f7b3569ec3847ccd9b8963ef1e` (tree
`c634a14f22e0a41867a3e234e7f750e4c49f7d4b`). Local qualification passed from a
clean detached worktree, including the complete original shared suite under
Miri. This is a bounded closed-enum decoding repair, not a claim of complete
unknown-field or kernel conformance.

The [original typed-collection recovery](shared-map-recovery.md) remains a
historical record for `c89608bd`: 233 original tests plus three regressions.
Independent review subsequently found the decoding gap below. This follow-up
adds three generated-API regressions, for six additional regressions in total.

## Reproduction and repair

Using the unmodified generated `TestAllTypes` API:

```rust
use pbrs::Parse;
use rust_out_shared::unittest_rust_proto::TestAllTypes;

let message = TestAllTypes::parse(&[
    0x98, 0x03, 1, 0x98, 0x03, 42, 0x98, 0x03, 2,
]).unwrap();
let values = message.repeated_nested_enum();
```

Before the repair, `len()` reported three, `get(1)` returned `None`, and iteration
stopped after the first valid value, hiding the final valid value. The decoder
stored unknown closed-enum numbers as ordinary collection elements; the checked
typed codec then rejected one during iteration. MiniTable construction had
discarded the closed/open distinction and supplied enum metadata.

The retained [pre-fix failing regression](closed-enum-recovery/repro-pre-fix.txt)
reports the first mismatch (`len() == 3`, expected two). It ran from the
`f8eec452`-based integration worktree with the new regression added, before the
repair; it is explicitly not a clean committed-source qualification.

The upstream fixture is `rust/test/unittest.proto`: edition 2023 with
`enum_type = CLOSED` and `repeated_field_encoding = EXPANDED`, giving
proto2-compatible semantics. It is not a `syntax = "proto2"` fixture.

The fix restores the valid typed-storage invariant at the decoding boundary:

- MiniTables distinguish open and closed enum fields and link closed-field enum
  metadata in field order. The pinned enum mini-descriptor format is decoded
  into sorted `u32` wire patterns, including sparse and negative enum numbers.
- Singular and repeated closed-enum numbers are validated before presence or
  collection storage changes. Rejected numbers enter the message's unknown
  fields with their full original `u64` varint value. Known values after an
  unknown remain visible; a singular unknown does not clear a previous value.
- Packed and expanded enum inputs are accepted independently of the field's
  declared packing preference. Open enums continue to preserve unknown numbers
  as visible enum values.
- A map entry containing an unknown closed-enum value, or a wrong-wire enum
  value, is retained as one unknown length-delimited parent field. Rejection
  stays set even if a later value in that entry is valid. The entry cannot
  replace an existing valid value for the same key; its original payload is
  preserved. An omitted enum map value defaults to zero.

Repeated iteration is unchanged: `next()` and `size_hint()` remain O(1), with
no read-time filtering or index remapping. Correct length, indexing, mutation,
and `ExactSizeIterator` behavior follow from rejecting invalid closed values
before storage. The typed codec remains checked; no size-based enum cast or
integer-to-Rust-enum reinterpretation was introduced.

Enum MiniTables own their parsed metadata in a `Box`; linked raw pointers must
outlive the referring message tables and messages. Generated `OnceLock` tables
retain that metadata for their process lifetime. Test-owned tables are reclaimed
only after their arenas and views are no longer used. Strict-provenance Miri
runs retain leak checking. Independent read-only review found no blocking
issues in the frozen source, including metadata linking, wrapping descriptor
arithmetic, rejection order, and fixture ownership.

## Regression coverage

The three new generated-API tests cover:

- Packed and expanded valid/unknown/valid sequences; consistent `len`, `get`,
  iteration and exact-size accounting; mutation; negative values; clearing
  visible values while retaining unknown wire values; unknown-only input.
- Singular default/no-presence behavior; known then unknown behavior; full-width
  `u64` preservation; required closed unknown rejection and required open
  unknown acceptance and round-trip preservation.
- All named values in a sparse signed enum descriptor, plus rejection and
  unknown preservation for unnamed values including integer extremes.

Core tests exercise descriptor parsing and a real decoded, linked closed-enum
map fixture: valid key then unknown same key; unknown then valid within one
entry; wrong-wire enum value; omitted value; exact unknown-entry payload
retention and round trip. This map fixture is a kernel test, not a newly
generated upstream schema. The original 19 upstream assertion files and
generated sources were not edited.

## Exact-source qualification

The detached worktree
`/Users/mingley/dev/pure-protobuf-qg05-closed-qualification-20260929` was clean
before and after all checks. The pinned upstream source was also clean.

| Check | Result | Transcript |
| --- | --- | --- |
| Original shared consumer suite | 19 crates, 233 original tests + 6 regressions passed | [normal shared](closed-enum-recovery/shared-3a7aa128.txt) |
| Core tests | 96 library + 38 native shared + 26 runtime + 18 kernel passed | [normal core](closed-enum-recovery/core-3a7aa128.txt) |
| Shared suite under Miri | 233 original tests + 6 regressions passed | [Miri shared](closed-enum-recovery/miri-shared-3a7aa128.txt) |
| Core under Miri | 96 library + 26 runtime + 18 kernel passed | [Miri core](closed-enum-recovery/miri-core-3a7aa128.txt) |
| Warning-strict Clippy | Passed | [Clippy](closed-enum-recovery/clippy-3a7aa128.txt) |
| Warning-strict rustdoc | Passed | [rustdoc](closed-enum-recovery/rustdoc-3a7aa128.txt) |
| Touched-source formatting and whitespace | Passed | [command record](closed-enum-recovery/summary-3a7aa128.json) |

The [summary JSON](closed-enum-recovery/summary-3a7aa128.json) records exact
commands, per-suite counts, environment, source/tool identity, clean-worktree
checks, and SHA-256 hashes for all seven retained transcripts.

Local host: `aarch64-apple-darwin`; Rust `1.98.1` (`48a229cea`, LLVM 22.1.8);
Cargo `1.98.1` (`797e8a9bc`). Local Miri: `0.1.0` (`e7769602ac`), using nightly
Rust `1.100.0` (`e7769602a`, LLVM 23.1.0) and nightly Cargo `1.100.0`
(`e8cb624d5`). This local result is not a 32-bit or big-endian qualification.

The compiler was the existing pinned `libprotoc 35.1`, built from protobuf
`35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03` (`v35.1`), with SHA-256
`e2b116ef44d4b7f3246945ceb1938c72f04e16040020e321ac601869135ab940`.
The ignored `third_party/protobuf` symlink resolves to that clean source in the
main checkout; source and compiler were reused without modification. Generator
flags remain `--rust_out --rust_opt=experimental-codegen=enabled,kernel=upb`.

Commands from the detached worktree:

```sh
export PATH=/opt/homebrew/bin:$PATH
export PROTOC=/Users/mingley/dev/pure-protobuf/target/conformance-build/protoc-35.1.0
export CARGO_BUILD_JOBS=2 CARGO_TERM_COLOR=never
export CARGO_TARGET_DIR=/Users/mingley/dev/pure-protobuf-audit-20260929/target/qg05-exact
bash scripts/test-rust-out-shared.sh
cargo test -p pbrs --lib --test runtime --test upb_kernel --test google_shared
cargo clippy -p pbrs --lib --test runtime --test upb_kernel --test google_shared -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc -p pbrs --no-deps --lib
export CARGO_TARGET_DIR=/Users/mingley/dev/pure-protobuf-audit-20260929/target/qg05-miri
export MIRIFLAGS='-Zmiri-disable-isolation -Zmiri-strict-provenance'
cargo +nightly miri test --manifest-path rust_out_shared/Cargo.toml --lib --tests
cargo +nightly miri test -p pbrs --lib --test runtime --test upb_kernel
```

At the same source SHA, Linux
[compatibility CI](https://github.com/mingley/pure-protobuf/actions/runs/36622048393)
and [kernel Miri CI](https://github.com/mingley/pure-protobuf/actions/runs/36622048523)
passed. The latter covers 96 library, 26 runtime and 18 kernel tests with strict
provenance on nightly `1.101.0` (`c1070d693`, 2026-09-28); it is separate from
the complete shared-suite Miri run above. Its unavailable h2 test step was
skipped and contributes no coverage claim. The broader
[checkpoint CI](https://github.com/mingley/pure-protobuf/actions/runs/36622116837)
was still in progress when this record was prepared.

General ordinary unknown-field handling and non-enum map-entry unknown-field
routing remain unchanged and outside this repair. These results establish the
tested closed-enum storage, presence, collection and unknown-wire behaviors,
not full unknown-field/kernel conformance.
