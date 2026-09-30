# PK-23: map dev-loop cells

## Implementation

The dev-loop codec registry now contains generated-message workloads for
`map_8` and `map_64`. Each size has three deliberately separate cells:

- `parse_touch` parses the wire into `TestAllTypesProto3` and visits every map
  entry inside the measured window;
- `get` looks up the final key in the already-parsed generated message;
- `iter` visits every entry in the already-parsed generated message.

Fixture construction, serialization, and the existing cross-codec preparation
happen before the allocation/timing window. The cells consequently use the
same counting allocator and child/report protocol as all other codec cells.
The fixture unit test checks both cardinalities, final-key lookup, and that the
parsed message has the same full-map checksum as the constructed message.

## Verification status

The focused command was attempted:

```text
cargo test --manifest-path bench/devloop/Cargo.toml map_cells::tests -- --nocapture
```

It did not reach compilation of the dev-loop crate. `v4_tat` requires
`third_party/protobuf/src/google/protobuf/test_messages_proto3.proto`, which is
absent from this checkout, and its build script fails closed. Therefore no
exact allocation counts or before/after PK-17 comparison are claimed here.
PK-23 remains `in_progress` until the pinned protobuf source fixture is restored
and `devloop-compare` plus all six cells run successfully.
