# Owned message field checks

This consumer generates one schema with `owned_message_fields(true)`, with
explicit lazy storage, and with a fresh default configuration immediately after
the owned configuration. The default and explicit-lazy Rust output must match
exactly, so the owned setting cannot leak between generation requests.

```sh
cargo run --locked --manifest-path tests/proof/owned_messages/Cargo.toml
```

Rust and `protoc` must be available. The independent reference is Prost 0.14.4,
pinned by the lockfile. Valid parsed values are compared after pbrs serialization
and Prost decoding. Separate checks preserve clone independence, merging,
unknown fields, failed-merge state, proto2 legacy bytes, proto3 UTF-8 rejection, unread truncation,
recursion limits, required-field merging, oneof replacement, maps and groups.

A scalar-and-child node also checks the owned profile's smaller metadata layout.
The 64-node fixture distinguishes the work each profile performs: owned
parsing allocates one box per actual child and later traversal allocates nothing;
lazy/default parsing defers child construction. These are deterministic work
checks, not speed thresholds. CPU, allocation bytes and encoding costs are
measured separately in the benchmark campaigns.

The build also generates `codec_cases.rs` with the public owned API. A campaign
can compare that artifact with its frozen generator variant to bind its timing
results to the shipped configuration path. The API and profile tradeoffs are
explained in the [codegen guide](../../../docs/guides/codegen.md).
