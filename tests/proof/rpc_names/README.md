# RPC service namespace consumers

This runner compiles legal service names that shadow framework traits, types,
constructors, crates and generated generic/local names. The full inventory is
in [services.txt](services.txt): 55 names, with all four RPC shapes per service.
It preserves public service identifiers and explicit user type/codec paths.

Install Rust, Python 3 and `protoc`, then run from a full repository checkout:

```sh
cargo fetch --locked
python3 tests/proof/rpc_names/run.py --out work/parity-audit/rpc-names
```

The default run builds the workspace with all features, using locked offline
dependencies, and reads Cargo's artifact receipt. It generates and compiles
55 consumers for each of native, Tonic, native compat and prost-native output.
Twelve more consumers check runtime aliases, codec aliases, user documentation
and attribute literals, and extern paths containing `pbrs` as a user component.
They use ordinary external crate aliases, absolute aliases and crate-local
modules, with warnings denied and unsafe code forbidden in the consumer.
Sixty-one bridge consumers check native and adapter crate aliases, explicit
codec precedence, unchanged user paths and literals, setter replacement,
parallel descriptor workers, thread isolation and reset to defaults. They cover
bare, absolute, `crate::`, `self::` and `super::` paths. The standalone Prost
generator keeps its independent default native runtime path after configured
generation.
A separate consumer uses `tonic-prost-build` 0.14.6 with the pbrs codec and real
adapter messages as a reference generator baseline.

To reuse an existing compatible all-feature build:

```sh
cargo build --locked --offline --workspace --all-features --message-format=json > work/rpc-artifacts.jsonl
python3 tests/proof/rpc_names/run.py --artifacts work/rpc-artifacts.jsonl --out work/rpc-names
```

Use a fresh output directory. Commands, compiler diagnostics and generated
sources are retained there, with selected input hashes and service-inventory
source hashes. Any candidate compile failure or changed selected input fails
the runner.

`--baseline-plugin` optionally records a historical generator's native, Tonic
and compat results without treating its known failures as candidate passes.
Previously valid service names must stay unchanged. `--prebuilt` accepts the
same recorded plugin/dependency paths for a local metadata-only replay;
`--skip-prost` explicitly limits such a local run when its cached native
runtime lacks the `prost` feature. That subset is recorded in the report.
The default CI run includes prost-native output and all bridge consumers.
`--only-bridge-aliases` records just the 61 bridge controls for a bounded local
replay; it excludes the service inventory and reference generator baseline.

These are generated-source metadata consumers. Native/Tonic/compat messages
are explicitly mapped to real runtime DTOs to isolate service templates from
message generation; prost-native consumers derive their own Prost messages.
The runner does not execute handlers or RPCs, establish arbitrary schema or
method-name compatibility, or measure runtime and generation performance.
