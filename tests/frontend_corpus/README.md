# Pinned frontend differential corpus (GN-08)

One entry per `.proto` file across the six card-mandated sources plus two
pinned support legs, with a harness (`scripts/frontend-diff.sh`) that records
pinned-protoc descriptor output and diffs a frontend against it in `parse`,
`link` and `full` modes. Route-independent per GN-05 (upstream-first protox
contribution): the harness runs baseline-only with no frontend present, so
GN-06 and later cards adopt it immediately.

## Layout

```text
tests/frontend_corpus/
├── README.md            # This document
├── EVIDENCE.md          # GN-08 acceptance evidence
├── corpus.json          # Pins, include roots, one entry per file (generated)
├── diff.py              # Harness driver (invoked by scripts/frontend-diff.sh)
├── fetch-corpus.sh      # Reproducible regen from pinned upstreams
└── proto/               # Vendored .proto bytes (184 files, 1.7 MiB)
    ├── protobuf/        # 80 files: every src/google/protobuf/*.proto,
    │                    #   compiler/*.proto, + the two java feature protos
    ├── googleapis/      # 39 files: google/api/*.proto, google/rpc/** (top)
    ├── envoy/           # 22 files: type/v3, annotations, core/v3, trace/v3
    ├── otel/            # 10 files: whole opentelemetry/proto tree
    ├── grpc/            # 26 files: whole grpc-proto grpc/ tree
    ├── xds/             # 6 support files: udpa/annotations + xds/ (envoy deps)
    └── pgv/             # 1 support file: validate/validate.proto (envoy dep)
```

Edition 2024 fixtures (9 valid plus 8 rejected and 1 support file) are referenced in place at
`tests/fixtures/edition2024/` and hashed into `corpus.json` like the rest;
they are not copied. Total: **202 entries (194 expect-ok, 8 expect-fail)**.

## Pins and licenses

Recorded in `corpus.json` (`repos`); every fetch verifies the SHA before any
file is copied, and the harness verifies every vendored byte against the
manifest hash before recording a baseline.

| Leg | Upstream | Pin | License |
|---|---|---|---|
| protobuf | protocolbuffers/protobuf | `v35.1` (`35cd01f9…`) | BSD-3-Clause |
| googleapis | googleapis/googleapis | `5d2a5100…` | Apache-2.0 |
| envoy | envoyproxy/data-plane-api | `e23a28a4…` | Apache-2.0 |
| otel | open-telemetry/opentelemetry-proto | `v1.9.0` (`a8951735…`) | Apache-2.0 |
| grpc | grpc/grpc-proto | `81333082…` | Apache-2.0 |
| xds (support) | cncf/xds | `dba9d589…` | Apache-2.0 |
| pgv (support) | envoyproxy/protoc-gen-validate | `414042a5…` | Apache-2.0 |

The protobuf leg reuses the repository `vendor/google` pin and is fetched by
`scripts/fetch-protobuf.sh`. The support legs exist because every envoy file
imports `udpa/annotations/*.proto` and usually `validate/validate.proto`,
neither of which ships in data-plane-api; they are pinned exactly like the
corpus legs (cf. the repo's own `buf.lock` dependency set).

## Usage

```sh
./scripts/frontend-diff.sh --corpus pinned            # baseline only (the gate)
./scripts/frontend-diff.sh --corpus pinned --frontend <bin>
./scripts/frontend-diff.sh --corpus pinned --mode parse --entry 'otel:opentelemetry/proto/trace/v1/trace.proto'
./scripts/frontend-diff.sh --list
```

Baseline output lands in `target/frontend-diff/baseline/<mode>/` (per-entry
`.status`, plus `.fds` for link/full) with a machine-readable
`summary.json`. Compare mode adds `target/frontend-diff/frontend/` and
reports verdict/byte mismatches. Exit codes: 0 clean, 1 mismatch, 2 usage or
environment error. The corpus is vendored, so comparison needs no network
fetch. Compiler setup is separate: `scripts/build-pinned-protoc.sh` can fetch
pinned sources and needs CMake plus a C++ compiler on the first run. Prepare
that cache before expecting an offline run.

## Modes

| Mode | protoc invocation | Oracle |
|---|---|---|
| parse | `--descriptor_set_out` to scratch | accept/reject verdict only |
| link | `--descriptor_set_out` | verdict + `FileDescriptorSet` bytes |
| full | `--descriptor_set_out --include_source_info --retain_options` | verdict + full-fidelity bytes |

`parse` deliberately compares verdicts, not diagnostics text: a correct
frontend must accept exactly what pinned protoc accepts, including the 8
rejected Edition 2024 fixtures. `link` proves import/name resolution;
`full` additionally proves source locations and option fidelity.

## Frontend contract (for GN-06 and later)

```
BIN --mode parse|link|full -I DIR... [--out PATH] FILE...
```

- `parse`: exit 0 iff pinned protoc would accept every `FILE`; `--out` is
  omitted and no output is compared.
- `link`: on success write the `FileDescriptorSet` (no source info) to
  `--out` and exit 0; on failure exit nonzero.
- `full`: as `link` but with source info and retained options, matching
  `protoc --include_source_info --retain_options` byte for byte.
- `FILE` arguments are resolved against the `-I` roots exactly as protoc
  does; the working directory is the repository root.

Comparison is byte-exact in `link`/`full`. If a later card needs normalized
comparison (e.g. field order inside options), it extends `diff.py` — GN-08
records the strict baseline.

## Regeneration and audit

```sh
./tests/frontend_corpus/fetch-corpus.sh   # re-fetch pins, rewrite proto/ + corpus.json
git status --porcelain tests/frontend_corpus  # must be empty afterwards
```

To rotate a pin, edit the SHA in `fetch-corpus.sh`, re-run it, and review the
`corpus.json` + `proto/` diff. To extend a leg (e.g. deeper `google/api`
subpackages), extend the copy lists in `fetch-corpus.sh`; every new file
becomes an entry automatically after regen.
