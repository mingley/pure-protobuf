# Protoc plugin response encoding

The generator now writes each file directly into one sized response buffer.
Previously, it encoded each file into a temporary buffer and copied that
buffer into the response. The change covers protoc's
`CodeGeneratorResponse` path. [Source](https://github.com/mingley/pure-protobuf/commit/8e435ed5608a1f826392d6816e96c65d1027e945)

The screen used Rust 1.100.0 and eight paired ABBA blocks per workload. A
CPU ratio below one means the candidate used less CPU. Allocation requests
include reallocations.

| Response | Candidate / base CPU | Allocation requests, base → candidate |
| --- | ---: | ---: |
| Small | 0.468 | 16 → 1 |
| 1,000 messages, lean | 0.516 | 73 → 1 |
| 1,000 messages, default | 0.953 | 73 → 1 |
| File with alias | 0.360 | 15 → 1 |

The probe compared complete response bytes, including alias order, Unicode
and lengths on both sides of varint boundaries. The measurements cover
serializing already-generated source. For generation, file I/O and consumer
build times, use the full harness. [Raw samples](https://github.com/mingley/pure-protobuf/actions/runs/44411951788/artifacts/14477996013)
· [Generator benchmark](../../bench/codegen/README.md)
