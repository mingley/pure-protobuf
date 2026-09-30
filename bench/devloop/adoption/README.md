# Public adoption corpora

This standalone consumer qualifies the SB-26 schemas and complete read walks.
The [matrix contract](../ADOPTION.md) defines the later timing workloads.
Every fixture is synthetic. There are no private schemas or captured payloads.

Build prerequisites are Rust 1.88+, an ordinary `protoc` compatible with
prost-build 0.14, and the existing pbrs source tree. This consumer does not
require Google's Rust/upb compiler or the repository's pinned conformance
checkout. Its reviewed manifest reuses pbrs, prost/prost-build 0.14 and
serde_json from the established benchmark graphs; the lockfile resolves prost
0.14.4. Shipping manifests and default graphs are unchanged.

```sh
# Reproduce schemas and generated fixture/field-walk source (Python + rustfmt).
python3 bench/devloop/adoption/generate.py
python3 bench/devloop/adoption/generate.py --check

# Compile both generators' actual messages, including twenty options files.
cargo test --locked --manifest-path bench/devloop/adoption/Cargo.toml
cargo clippy --locked --manifest-path bench/devloop/adoption/Cargo.toml \
  --all-targets -- -D warnings

# Run all equality/read checks and emit the deterministic inventory JSON.
cargo run --locked --manifest-path bench/devloop/adoption/Cargo.toml \
  --bin inventory
```

Use an external `CARGO_TARGET_DIR` when building this excluded consumer.
Generated pbrs/prost messages remain in Cargo's output directory. The fixture
script generates the wide schema, its two independent field walks and the
options schemas/tests; their checked-in outputs must only change through that
script. `--check` also verifies formatting using the installed rustfmt, whose
version is recorded in qualification evidence.

There are 64 inventory specimens: 48 recursive queries, three entity lists,
ten sparse presence patterns and three map sizes. Full equality is checked by
decoding pbrs's wire output into the independent generated prost type. Two
separate field walks must also agree. Map byte order may differ; equality and
read checksums are independent of map iteration order. Every byte of strings
and bytes is consumed. Any values are also decoded as the generated public
Payload type and all their inner fields are consumed by both codecs.

The tests cover all sparse fields with explicitly present defaults, every
typed oneof's present default, exact query depths, all predicate kinds,
modified bytes in the last entity and changed/reordered map entries. Every
options-file consumer also cross-decodes all sixteen fields. Compiling and
testing these fixtures establishes semantic equivalence, not a performance
result. SB-26b still owns the registered codec/RPC measurements, cold first RPC,
codegen/check timings and P1-P5 baseline. See [evidence](evidence/qualification.json).
