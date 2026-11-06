# Generated defaults with a forbid-unsafe consumer

`Config::safe_message_defaults(true)` derives each message's default from its
field types and removes the generated module's unsafe-code allowance. The
default generation profile keeps its existing initialization strategy. Both
profiles retain the same storage layout, accessors and wire format.

This consumer uses `#![forbid(unsafe_code)]`. Its generated messages exercise
proto2 declared defaults, nonzero enum defaults, required fields and groups;
proto3 implicit and explicit presence; strings, bytes, repeated fields, maps,
oneofs and nested messages; cold storage; cloning, clearing, shared input,
malformed input and unknown-field roundtrips. Lazy, globally owned and selected
boxed nested-message profiles run the same checks.

The policy applies to the generated consumer, including macro expansions.
The runtime is a dependency with its own implementation and lint policy;
this does not claim that every dependency contains no unsafe code.

Run from the repository root with Rust, Cargo, Python and `protoc` installed.
Fetch dependencies before using the offline runner:

```sh
cargo fetch --locked --manifest-path tests/proof/safe_defaults/Cargo.toml
python3 tests/proof/safe_defaults/run.py --out work/safe-default-consumers
```

Use a new output directory for each run. The runner records all commands,
statuses and selected source hashes. The three positive profiles each print
four checks. The legacy negative control must fail because the same consumer
forbids the ordinary generated unsafe initialization.

The individual positive commands are:

```sh
cargo run --locked --manifest-path tests/proof/safe_defaults/Cargo.toml
cargo run --locked --manifest-path tests/proof/safe_defaults/Cargo.toml --features owned
cargo run --locked --manifest-path tests/proof/safe_defaults/Cargo.toml --features boxed
```

`ordinary-control` is a measurement-only feature that permits the ordinary
generated defaults in this fixture. `forbid-legacy` preserves the consumer's
forbid policy but selects ordinary output, so checking that feature is an
intentional compile failure. Neither feature changes the published runtime.

Per-field initialization can change construction and downstream build costs.
The fixture also accepts `default`, `empty_encode`, `parse` and `read_all`
followed by an iteration count for paired local diagnostics. `read_all`
touches all populated fields and payload bytes and checks its final checksum.
Keep ordinary and safe builds separate, preserve every losing row, and do
not interpret these probes as a full tonic/Prost comparison or ranking.
