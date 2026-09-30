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

## Verification

Source-level OTLP text round-trip coverage passes:

```text
cargo test --test generated_text generated_otlp_text_round_trips_nested_enums
# 1 passed
```

The dev-loop unit contract passes with the pinned protobuf 35.1 compiler:

```text
cargo test --manifest-path bench/devloop/Cargo.toml text_cells::tests -- --nocapture
# 1 passed
```

Two consecutive 20-iteration, three-repeat harness runs returned identical
exact allocation results (setup and semantic validation remain outside the
measurement window):

| Cell | allocations/op | allocated bytes/op |
|---|---:|---:|
| TAT encode | 20 | 5,846 |
| TAT decode | 136 | 9,264 |
| OTLP encode | 21 | 4,909 |
| OTLP decode | 76 | 7,152 |

The identical rerun makes the PK-21 allocation result CI-legible and
reproducible rather than leaving it in the original ad-hoc probe. The JSON
reports are local measurement artifacts (`target/pk24-text.json` and
`target/pk24-text-rerun.json`); the stable commands and exact results above are
the retained evidence.
