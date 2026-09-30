# PK-24: text encode/decode dev-loop cells

## Cell contract

The dev-loop registry now includes four generated-message text-format cells:

| Cell | Corpus | Timed operation | Result consumed |
|---|---|---|---|
| `codec.pbrs.text.tat_encode` | protobuf TAT proto3 | `TestAllTypesProto3::to_text` | text byte length |
| `codec.pbrs.text.tat_decode` | protobuf TAT proto3 | `TestAllTypesProto3::from_text` | encoded message length |
| `codec.pbrs.text.otlp_encode` | OpenTelemetry trace v1 | `TracesData::to_text` | text byte length |
| `codec.pbrs.text.otlp_decode` | OpenTelemetry trace v1 | `TracesData::from_text` | encoded message length |

The OTLP type is generated at build time from the checked-in public corpus at
`bench/corpora/otlp/protos/opentelemetry/proto/trace/v1/trace.proto`; it is not
a handwritten stand-in. Fixture creation and semantic round-trip checks run
before the timing/allocation window. The existing dev-loop child-process
protocol reports exact allocation count and allocated bytes for every cell.

## Verification status

Source-level OTLP text round-trip coverage passes:

```text
cargo test --test generated_text generated_otlp_text_round_trips_nested_enums
# 1 passed
```

A local dev-loop build currently stops in the existing `v4_tat` peer build
before compiling the dev-loop binary because `protoc-gen-rust` is not installed
on this host. This is independent of the pbrs text cells, but means no honest
PK-21 performance/allocation reproduction can be recorded yet:

```text
cargo test --manifest-path bench/devloop/Cargo.toml text_cells::tests --no-run
# v4_tat build.rs: protoc-gen-rust: program not found or is not executable
```

PK-24 remains in progress until the four registered cells are run on a host
with the SB-08 peer toolchain and their exact allocation results are compared
with the PK-21 probe.
