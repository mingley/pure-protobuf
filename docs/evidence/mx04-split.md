# MX-04: split `src/runtime.rs` into `src/runtime/` — verification record

Base: `f7ff9a7f`. `src/runtime.rs` (2523 lines) is now a 69-line hub
declaring 9 private submodules and re-exporting the exact prior public
surface. `src/lib.rs` unchanged.

## Layout

| File | Lines | Contents |
|---|---|---|
| `src/runtime/arena.rs` | 138 | `Arena`, `ArenaInner`, allocators, fuse |
| `src/runtime/array.rs` | 130 | `InnerRepeatedMut`, `empty_array`, `kernel_array_push`, repeated get/set |
| `src/runtime/decode.rs` | 364 | `parse_into`, field/map/packed decoders, `ClearAndParse` + `MergeFrom` |
| `src/runtime/encode.rs` | 323 | `encode_msg`/`encode_slot`, key/scalar/packed helpers, `Serialize` |
| `src/runtime/extension.rs` | 10 | `MiniTableExtensionPtr`, `ExtensionRegistryPtr` (UK cards fill this in) |
| `src/runtime/layout.rs` | 774 | `MsgData`, `FieldKind`, `MessagePtr`, owned/mut/view inners, copy/clone, `Clear` + `CopyFrom` + `TakeFrom` |
| `src/runtime/map.rs` | 177 | `InnerMapMut`, `empty_map`, `kernel_map_*`, key/value kind |
| `src/runtime/mini_table.rs` | 304 | `MiniTable`/`MiniField`/`FieldType`, base92 mini-descriptor decode, build/link, `__unstable` |
| `src/runtime/reflect.rs` | 439 | `Associated*`/`UpbGet*`/`Kernel*` traits, view/mut conversions, `message_set_*`, `message_eq` |

Cross-module sharing is `pub(crate)` items via 4 hub glob re-exports
(`array`, `layout`, `map`, `reflect`); everything else resolves
through the explicit `pub use` list. The 51 `pub` items are re-exported
individually, no `pub mod`. No `pub` → `pub(crate)` downgrades: every
visibility change was private → `pub(crate)` (struct-literal fields,
slot accessors, `alloc_bytes`, `field_by_number`, `encode_slot`,
`retain/release_bytes`, `adopt_owned_msg`), all invisible outside the
crate. `encode_slot` is shared with unit tests through the private
module path (`crate::runtime::encode::encode_slot`) instead of a hub
glob so normal builds stay warning-free.

The 3 pre-existing unit tests moved with their code
(`runtime::decode::tests`, `runtime::layout::tests`,
`runtime::mini_table::tests`).

## Public items unchanged (accept 1, part 1)

`runtime` is `#[doc(hidden)]`, so instead of the MX-01 rustdoc-page
diff, a temporary integration probe (`tests/zz_mx04_probe.rs`, deleted
after use) imported all 51 public paths enumerated from the base file
and instantiated every concrete type plus the non-generic fns. Probe
passed before the split and passes after it; the generated consumers
(`rust_out_person`, `rust_out_shared`, `src/map.rs`, `src/repeated.rs`)
exercise ~40 of the paths in normal builds.

## Behaviour unchanged (accept 1, part 2)

- `shared-out` (`PROTOC=target/pinned-protoc-build/protoc
  ./scripts/test-rust-out-shared.sh`): all 19 upstream shared crates
  pass, 233 passed / 0 failed — same as base.
- `rust_out_person` standalone `cargo test`: builds against the split,
  all suites pass.
- `core-lib` (`cargo test -p pbrs --lib`): 43/43, same count as base.

## upb-kernel check (accept 2)

`cargo test -p pbrs --test upb_kernel`: 16/16 across
`tests/upb_kernel.rs` + `tests/upb_kernel/{arena,array,decode,encode,
extension,layout,map,mini_table,reflect}.rs`. Real public-API tests
where the kernel surface permits (table build/link, arena fuse,
string views, empty collections, proto-string round-trip, pointer
shapes); `decode`/`encode` hold documented placeholders until UK cards
land parseable fixtures. The hub header documents the target as the
`upb-kernel` check.

## Checks

- `cargo fmt --check`: clean (workspace-wide)
- `cargo clippy -p pbrs --lib --all-features`: clean
- lib + test builds: zero warnings (baseline was also zero)
