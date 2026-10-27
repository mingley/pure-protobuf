# Prost wire behavior proof

This standalone consumer compiles the same two schemas with pbrs and
prost-build 0.14.4, then records one JSON line per wire case. It compares
acceptance and the resulting message, including merge into a populated message.
It is a finite differential test, not a proof of every possible protobuf input
or a source-compatible replacement for `prost::Message`.

```sh
cargo run --locked --manifest-path tests/proof/prost_parity/Cargo.toml > prost-parity.jsonl
```

Install `protoc` or set `PROTOC` before running. The checked-in lockfile fixes
Rust dependencies. Record `protoc --version`, `rustc --version`, the repository
revision and generated schema hashes with the output for an evidence campaign.
The final JSON line reports the number of proto3 cases and discrepancies.
A proto3 discrepancy makes the process exit unsuccessfully; its full input
and both results remain in the output.

The corpus covers all scalar wire forms and boundaries, signed values, UTF-8,
opaque bytes, unknown fields and groups, open enums, explicit presence,
packed/unpacked repeated fields, singular-message merge, repeated children,
oneof case reuse/replacement, duplicate map keys, map values, malformed wire,
recursion boundaries, 256 deterministic generated wire messages, and merging
into an existing populated object. Recursion tests use each library's defaults.

For successful messages, both results are decoded into the same prost type
before comparison. Prost maps use `BTreeMap`, making subsequent encoding
order deterministic. Comparing those normalized encodings preserves floating
point bits, including NaN payloads and negative zero. This does not demand
identical original serialization order, unknown-field retention or caches.
Those properties need separate tests.

Two proto2 controls report missing required-field handling and unknown closed
enum handling separately. They expose behavioral differences instead of
silently defining pbrs or prost as the correct implementation. The required
control uses each library's ordinary default parse operation.

The current corpus contains 330 distinct proto3 inputs and 654 operations:
324 inputs are parsed and merged into an empty object, and six inputs merge
into a populated object. Two operations reproduce the same input discrepancy:
`0a00` supplies a length-delimited value for the declared `int32` field 1.
pbrs accepts and preserves it as an unknown field; prost rejects the wire type.
The empty proto2 required-message control also differs: pbrs rejects it while
prost accepts it. These results prevent an identical-behavior claim.

For a regression gate that checks these exact differences and all the other
controls, run:

```sh
cargo run --locked --manifest-path tests/proof/prost_parity/Cargo.toml -- --check-known-differences
```

This explicit mode succeeds only when the two named proto3 differences have
the recorded input, operation and acceptance directions, the required-field
control differs in the recorded direction, the closed-enum control agrees,
and every other corpus operation agrees. Its summary reports
`harness_passed: true` and `parity_equivalent: false`. The default strict run
continues to exit 1. A passing regression gate does not prove full parity.
