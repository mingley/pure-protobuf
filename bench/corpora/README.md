# Codec corpora (SB-05)

This directory contains the pinned schemas and deterministic payload generators
used by the codec scoreboard (categories A1–A15). It is for benchmark authors
who need reproducible protobuf inputs. Checked-in `tiny` and
`typical` payloads are stable; `large` and `huge` payloads are regenerated on
demand and verified by hashes in each manifest.

Corpora are test inputs, not performance evidence. Use the
[cross-language peer guides](../xlang/cpp/README.md) and
[benchmark contract](../../docs/benchmark-contract.md) to choose equivalent
operations and ownership before comparing timings. A generated payload or
successful round trip does not establish that every comparator ran that cell.

Each corpus vendorizes its `.proto` import closure at a pinned upstream commit.
`generate.py` builds descriptors with the pinned `protoc` and emits
length-tiered binary payloads that reproduce byte-identically from their
recorded seeds.

## Layout

| Path | Purpose |
|---|---|
| `<corpus>/protos/` | Vendored import closure; paths are import-relative, so pass `-I <corpus>/protos` to `protoc`. |
| `payloads/<corpus>/<message>/<tier>.bin` | Generated payloads. Only `tiny` and `typical` are checked in; `large` and `huge` are gitignored and regenerated on demand. |
| `manifest.json` | Per-corpus classification, upstream repos with commits and SPDX licenses, every file's SHA-256, and every payload's message, seed, size, and SHA-256. |
| `fetch.py` | Re-download pinned closures. `--verify` compares hashes without writing. |
| `reconstruct_go.py` | Rebuild google-messages schemas from pinned protobuf-go generated code and prove descriptor equivalence. |
| `generate.py` | Build payloads. `--verify` regenerates and compares hashes without writing. |

## Corpora

| Corpus | Upstream | Pin | License | Schemas |
|---|---|---|---|---|
| `otlp` | open-telemetry/opentelemetry-proto | v1.11.0 (`790608c4`) | Apache-2.0 | ResourceSpans, ResourceMetrics, ResourceLogs |
| `xds` | envoyproxy/envoy (+ cncf/xds, cncf/udpa, bufbuild/protoc-gen-validate, googleapis, protobuf WKTs) | v1.39.1 (`b579d07d`) etc.; see manifest | Apache-2.0 / BSD-3-Clause (WKTs) | Cluster, ClusterLoadAssignment, RouteConfiguration |
| `googleapis` | googleapis/googleapis (+ protobuf WKTs) | `5174d7c2` | Apache-2.0 / BSD-3-Clause (WKTs) | google.rpc.Status (+ ErrorDetails, annotations) |
| `grpc-testing` | grpc/grpc (repo `third_party` checkout) | `d1487957` | Apache-2.0 | SimpleRequest, SimpleResponse |
| `google-messages` | protocolbuffers/protobuf-go (generated code; schemas reconstructed — originals never published) | v1.36.12 (`cdd4c5f7`) | BSD-3-Clause | GoogleMessage1 (proto2+proto3), GoogleMessage2 |

## Classification (frozen)

`primary` corpora back the gated codec cells; `holdout` corpora are
fresh real-world shapes that catch overfitting (contract §3.5). The
split is recorded per corpus in `manifest.json` and per A-category in
`bench/scoreboard/categories.json`:

- `primary`: `grpc-testing` (these are the payload shapes the RPC
  benches already measure), `google-messages` (the canonical
  cross-implementation baseline).
- `holdout`: `otlp`, `xds`, `googleapis`.

## Tiers

| Tier | Band | Checked in |
|---|---|---|
| `tiny` | 0–63 B | yes |
| `typical` | 64 B–4 KiB | yes |
| `large` | 64 KiB–1 MiB | no (regenerate) |
| `huge` | 8–64 MiB | no (regenerate) |

## Regenerating

```bash
python3 bench/corpora/fetch.py                 # .proto closures
python3 bench/corpora/reconstruct_go.py        # google-messages schemas
python3 bench/corpora/generate.py              # all payloads
python3 bench/corpora/generate.py --verify     # determinism proof
```

Requirements:

- CPython 3.9+ with the `protobuf` runtime package.
- `protoc`: `$PROTOC`, else the repo's pinned 35.1 build, else `PATH`.

The recorded `protoc --version` is stored in `manifest.json` under
`config.protoc`. Payload bytes do not depend on the `protoc` version because
the descriptors only feed the Python runtime, which serializes deterministically.
