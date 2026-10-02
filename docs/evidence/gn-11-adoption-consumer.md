# GN-11 adoption consumer migration

The real standalone adoption consumer failed after multi-input Google type
ownership moved into the generated registry: its flat native include resolved
`crate::google::protobuf::Any` to the existing prost type, while `native::Any`
was absent. At base `40db064d3e8ff7a5d0e4f881956681f3cb7d123b`, the unchanged
consumer's locked, offline `cargo check` failed with 20 compiler errors. The
original source hashes and complete failure log are retained below.

The build script now explicitly maps native Google types to
`crate::native_google::protobuf`. It generates Any once from the persisted core
application descriptor set and descriptor types once from the persisted options
descriptor set, then compiles those owners in separate modules. Retaining the
application descriptor set preserves expanded Any JSON for `adoption.Payload`.
The 21 flat application includes contain no private Any/DescriptorProto copies.

The existing `native`, `google::protobuf`, `prost_types` and twenty
`options::part_i::{pbrs,prost}` namespaces remain. `native::Any`, `AnyView` and
`AnyMut` are aliases of the native owner. Schemas, fixture generators, corpus
recipes, registered cells, runtime/codegen source, manifests and lockfiles are
unchanged. Generated Rust remains in Cargo OUT_DIR; no binding was hand-edited.

## Validation

All commands use the shared pinned toolchain with `CARGO_BUILD_JOBS=1`, debug
information disabled and incremental compilation disabled. This is correctness
evidence; no compile-time, allocation or performance acceptance is claimed.

- Standalone `cargo test --locked --offline --manifest-path
  bench/devloop/adoption/Cargo.toml --target-dir target/base`: 14 passed. The new
  test covers native owned/view/mut aliases, independent prost cross-decoding,
  native descriptor roundtrip and expanded Any JSON roundtrip.
- Standalone all-target Clippy with `-D warnings`: passed.
- `python3 bench/devloop/adoption/generate.py --check`: passed.
- Actual inventory, codec-inventory and rpc-inventory executables: 64/512/64
  records; every retained field equals the committed qualification oracles,
  including wire lengths, complete-value/read checks and checksums. Existing
  current-source wire fingerprints remain present in emitted codec records.
- Completed compatibility/sharing/documentation suites: 11/6/25 passed. The
  broader run then completed 14 onboarding tests with one optional skip, but
  the remaining cold packaged test failed on shared disk exhaustion (ENOSPC).
  It exited 101; build/plugin suites were not reached. The complete failure is
  retained. The coordinator requested no redundant retry because integrated
  main gates cover those suites; no complete codegen pass is claimed here.
- Shipping codegen/runtime and bundled bindings are unchanged. The prior exact
  [GN-11 pinned regeneration check](gn-11-wkt-sharing.md) remains recorded;
  this benchmark-only follow-up did not repeat it. Integrated validation is
  owned by the coordinator.
- Standalone formatting and `git diff --check`: passed.

The existing optional toolchain tests retain their documented opt-in status.
Integrated official conformance remains the coordinator's gate. Original and
intermediate failed attempts are retained, including the scratch README link
checker failure (resolved by moving the draft to a non-Markdown snapshot).
The documentation checker was not changed.

## Retained proof

[Exact commands and exits](gn-11-adoption-artifacts/commands.json),
[tool/source/fixture hashes](gn-11-adoption-artifacts/provenance.json),
[generated owner proof](gn-11-adoption-artifacts/generated-ownership.json) and
[inventory comparison](gn-11-adoption-artifacts/inventory-comparison.json)
make the result auditable. [Raw artifacts](gn-11-adoption-artifacts/raw-artifacts.tar.gz)
contain original/development/final logs, source snapshots, both application
FDS files and all three emitted inventories; [member hashes](gn-11-adoption-artifacts/raw-member-sha256.json)
identify their exact bytes. Replay retained oracle equality without building:

```sh
python3 docs/evidence/gn-11-adoption-artifacts/check-inventories.py
```

Fresh Clippy output contained no stale imported-owner application files; its
canonical Any/descriptor sources matched the tested output byte-for-byte.
Completed consumer source/lock/fingerprint snapshots were also preserved in
local work before reclaiming caches. These correctness results do not close
codegen or adoption performance acceptance.
