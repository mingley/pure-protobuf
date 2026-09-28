# Codec corpora (SB-05)

Pinned, license-checked schemas plus deterministic seeded payload
generators for the codec scoreboard (categories A1–A15). Each corpus
vendorizes its `.proto` import closure at a pinned upstream commit;
`generate.py` builds descriptors with the pinned protoc and emits
length-tiered binary payloads that reproduce byte-identically from
their recorded seeds.

## Layout

- `<corpus>/protos/` — vendored import closure, paths import-relative
  (pass `-I <corpus>/protos` to protoc).
- `payloads/<corpus>/<message>/<tier>.bin` — generated payloads.
  Only `tiny`/`typical` are checked in; `large`/`huge` are gitignored
  and regenerated on demand (hashes in the manifest prove equality).
- `manifest.json` — per corpus: classification, upstream repos with
  commits and SPDX licenses, every file's SHA-256, every payload's
  message/seed/size/SHA-256.
- `fetch.py` — re-download the pinned closures (`--verify` compares
  hashes without writing).
- `reconstruct_go.py` — rebuild the google-messages schemas from the
  pinned protobuf-go generated code and prove descriptor-equivalence.
- `generate.py` — build payloads (`--verify` regenerates and compares
  hashes without writing).

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

Requirements: CPython 3.9+ with the `protobuf` runtime package, and
protoc (`$PROTOC`, else the repo's pinned 35.1 build, else `PATH`).
Recorded `protoc --version` is stored in `manifest.json` under
`config.protoc`; payload bytes do not depend on the protoc version
(the descriptors only feed the Python runtime, which serializes
deterministically).
