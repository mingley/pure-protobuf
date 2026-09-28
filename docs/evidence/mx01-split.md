# MX-01: split `src/codegen.rs` into `src/codegen/` — verification record

Base: `1782f678`. `src/codegen.rs` (9141 lines) is now a 160-line hub
declaring 11 private submodules and re-exporting the exact prior public
surface. `src/lib.rs` unchanged.

## Layout (all under the ~2000-line cap)

| File | Lines | Contents |
|---|---|---|
| `src/codegen/config.rs` | 1219 | `CodegenError`, `Stubs`, plugin options, `Config`, build helpers, thread-local codegen state |
| `src/codegen/descriptors.rs` | 906 | request/FDS/compile entry points, pool/file helpers, response encoding |
| `src/codegen/naming.rs` | 659 | idents, types, enums names, storage predicates |
| `src/codegen/messages.rs` | 1299 | docs, `emit_message`, accessors, enums |
| `src/codegen/parse.rs` | 816 | `emit_codec` driver, merge/validate, map decoders |
| `src/codegen/encode.rs` | 465 | size/write, packed, map encoders |
| `src/codegen/json.rs` | 716 | JSON leaf helpers + emitters |
| `src/codegen/text.rs` | 418 | text leaf helpers + emitters |
| `src/codegen/reflection.rs` | 82 | FDS bytes emission, reflection state |
| `src/codegen/native_stubs.rs` | 1913 | kernel (`::pbrs_grpc`) service/client/server |
| `src/codegen/tonic_stubs.rs` | 586 | tonic service/client/routes |

Cross-module sharing is `pub(crate)` items via hub glob re-exports; the
11 `pub` items are re-exported individually (`pub use`), no `pub mod`.

## Byte-identical output (accept 1)

Both regen scripts run against base SHA and after the split, outputs
compared with `diff -r`:

- `scripts/regen-generated.sh` → `src/generated/**`: IDENTICAL
- `scripts/regen-google-unittest.sh` → `tests/google_gen/**`: IDENTICAL

Note: both scripts show large pre-existing drift versus the checked-in
tree (flat-vs-hierarchical layout era); the criterion was therefore
verified as base-output == after-output, not tree-clean. Fresh
`src/generated` outputs do not compile against the tree (`mod.rs`
declares the hierarchical layout while `gencode.rs` expects flat
modules) — also pre-existing; regen comparisons restored the tree
between runs. Raw outputs were compared in `/tmp` and not committed.

Caught by this check: the first split attempt regex-publicized a
template line (`mod {gen_mod}` inside the generator's format string)
into `pub(crate) mod`, changing every regenerated file. Redone with
line-number-verified publicizing (exactly one known template impostor,
left untouched).

## Public items unchanged (accept 2)

`cargo doc -p pbrs --no-deps` before/after: same 11 item pages
(`CodegenError`, `Stubs`, `compile_protos`,
`encode_code_generator_response[_error]`,
`generate_from_code_generator_request`,
`generate_from_file_descriptor_set`, `Comments`, `Config`,
`SourceCodeInfo`, `SourceLocation`). Page HTML is identical except
`[src]` source-link targets (definitions moved files). Rustdoc also
emits redirect stubs under `codegen/config/` and `codegen/descriptors/`
for the new definition paths; those modules are private and unnameable.

## Checks

- `cargo fmt --check`: clean (workspace-wide)
- `codegen` (`plugin`, `pbrs_build`, `onboarding`): 14 + 26 + 41 pass
- `native-codegen` (`pbrs-grpc --test codegen`): 197 pass
- `tonic` (`-p protobuf-tonic`): all suites pass
- extra: `pbrs --lib` 40 pass (one fixup: hub tests now import
  `PathBuf` explicitly after `cargo fix` trimmed the hub prelude);
  `clippy -p pbrs --lib --all-features` clean;
  `bench/codegen` `test_run.py`: 25 tests OK
