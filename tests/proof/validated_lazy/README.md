# Validated lazy-message checks

This independent consumer checks lazy materialization after the ordinary parser
has validated the complete nested wire. It compares valid decoded values with
Prost 0.14.4 and checks that malformed input fails before a getter is called.

```sh
cargo run --locked --offline --manifest-path tests/proof/validated_lazy/Cargo.toml
cargo clippy --locked --offline --manifest-path tests/proof/validated_lazy/Cargo.toml -- -D warnings
```

Rust and `protoc` must be available. The lockfile pins the reference generator
and runtime. Dependencies must be cached before using `--offline`.

The build adds an atomic counter at every generated `validate_inner` entry.
This changes only the proof's generated source, leaving each validator body
unchanged. A 64-node parse must call 63 child validators; subsequent getters
must call none. These counters never appear in shipping benchmark binaries.

A separate allocator probe checks that the first child getter constructs one
immediate child, rather than eagerly constructing the remaining tree. Its
allocation count and requested bytes must be independent of whether four or
64 nodes remain. Full traversal still materializes the deferred children;
this proof does not assert that lazy and owned layouts have identical costs.

Additional controls cover:

- Generated default field layouts and method-name collisions.
- Truncated tags/lengths, invalid UTF-8 and recursion limits at public parse.
- Fresh merges after reads or mutation, including the ordinary partial state
  left by a failed merge.
- Clone independence, concurrent shared getters, retained unknown wire and
  caller-owned byte-buffer lifetime.
- Repeated, map and group descendants, required-field merging and oneof
  replacement. Ordinary collection helper fallback validation is recorded;
  this consumer does not assert that every collection path avoids revalidation.
- Old `MergeBytes` implementations that use the default checked fallback, new
  opt-in dispatch, and legacy constructor isolation.

The hidden `from_validated_wire` runtime helper is an internal generated-code
constructor. It records an existing semantic validation result; it does not
validate raw input. All public parse checks and ordinary merge paths remain
checked. No timing thresholds or throughput claims are part of this consumer.
