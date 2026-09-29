# xlang Go protobuf peers (SB-07)

Timing harnesses for three Go codec peers, driven by
`scripts/xlang-codec.sh`, on the same SB-05 corpora and devloop/1 JSON schema
as the C++/C peers:

| Peer | Implementation | Corpus binaries | Decode op | Encode op |
|---|---|---|---|---|
| `go` | `google.golang.org/protobuf` generated code (`proto.Unmarshal` / `MarshalAppend`) | `harness-go-<corpus>` | fresh message + parse per op | one message, serialize into a reused buffer per op |
| `go_vt` | vtprotobuf generated fast paths (`UnmarshalVT` / `SizeVT`+`MarshalToVT`) | `harness-go-vt-<corpus>` | fresh message + fast parse per op | one message, fast serialize into a reused buffer per op |
| `go_hyperpb` | Buf hyperpb dynamic parser (`CompileFileDescriptorSet` once, `proto.Unmarshal` per op) | `harness-hyperpb` (schema-generic, one binary) | fresh message + parse per op | not served (decode-only peer) |

Scoreboard placement (accept 2): `go` and `go_vt` are generated-code peers
for the typed categories (A1/A3/A5); `go_hyperpb` is compared only in the
dynamic-parse category (A12). The driver records every `go_hyperpb` encode
cell as `not_run` with the reason `hyperpb is decode-only; compared only in
A12 (dynamic/reflection decode)`, and the harness itself rejects `--op
encode` with exit code 3.

## Never shipped

TEST TOOL ONLY. This directory is built by `scripts/xlang-codec.sh` into
`target/xlang-codec/bin` (gitignored). No Cargo manifest, build script, or
shipped crate references it; it cannot enter a shipping dependency graph.

## Pins

`go.mod`/`go.sum` (both committed) pin every module; `go mod download all`
fetches them and every compile then runs with `GOPROXY=off`, so builds are
offline-reproducible after fetch.

| Module | Version | Zip hash (go.sum) | Used for |
|---|---|---|---|
| `google.golang.org/protobuf` | v1.36.12 | `h1:pJOKDDOyeXErUroCihFAd5LQuwXBSpVnKGrj5o/fwxc=` | `go` peer runtime; `protoc-gen-go` |
| `github.com/planetscale/vtprotobuf` | v0.6.0 | `h1:nBeETjudeJ5ZgBHUz1fVHvbqUKnYOXNhsIEabROxmNA=` | `go_vt` fast paths; `protoc-gen-go-vtproto` |
| `buf.build/go/hyperpb` (repo `github.com/bufbuild/hyperpb-go`) | v0.1.3 | `h1:wiw2F7POvAe2VA2kkB0TAsFwj91lXbFrKM41D3ZgU1w=` | `go_hyperpb` parser (default options, no PGO) |

Requires Go ≥ 1.25 (built with 1.25.3); the toolchain version is recorded in
the report provenance. The indirect closure (testify via hyperpb's test
deps, `x/text`, `genproto`) is compiled only where imported — never into the
timed binaries except through generated code.

## How it works

1. The driver builds the pinned `protoc`, then `make -C bench/xlang/go`
   builds `protoc-gen-go` and `protoc-gen-go-vtproto` from the pinned module
   cache and runs `codegen.sh`, which generates both plugins' output for
   every vendored `.proto` into `gen/<corpus>/` (gitignored, regenerated
   from the pinned inputs; a stamp skips it when nothing changed).
2. Generated Go import paths are M-mapped per file to
   `<module>/gen/<corpus>/p/<proto-dir>/<name>/<protopkg>`, where `<name>`
   is the `go_package` name when declared, else the dir basename, and
   `<protopkg>` is the proto package: sharing a dir no longer merges files
   that declare different Go or proto packages (the two `GoogleMessage1`
   variants, the WKTs). Output uses `paths=import` so each Go package lands
   in its own directory.
3. Well-known types are not generated in-tree; they M-alias to the upstream
   `google.golang.org/protobuf` WKT packages. This is vtprotobuf's designed
   layout (upstream field types, identical-layout casts inside the fast
   paths): generating WKTs in the same invocation makes the vtproto plugin
   emit broken `__.X` references, and its own WKT copies are vt-only shells
   without `ProtoReflect`, so aliasing to them would panic the standard
   peer. Both typed peers therefore share the upstream WKT type graph.
4. Two file classes are excluded from the *vtproto* invocation (they still
   get plain `--go_out`): service-bearing files (`test.proto`, which defines
   no messages — the plugin would emit gRPC stubs and drag in a
   `google.golang.org/grpc` dependency that floats the vtprotobuf pin) and
   group-bearing files (`message2.proto` — the plugin v0.6.0 emits
   non-compiling code for groups). Exclusions land in
   `gen/<corpus>/vtexcluded.go` with reasons; `go_vt` cells for those
   messages exit 3 (`not_run`) with the precise reason.
5. Each per-corpus binary links only its corpus types (via a generated
   blank-import register package) and resolves `--message` in the global
   registry, so one corpus's codegen or compile failure degrades to that
   corpus's `not_run` cells instead of killing the peer. `harness-hyperpb`
   instead loads the same `--desc` FileDescriptorSet the C++ peer uses and
   compiles the parser once, outside the timed loops.
6. Every cell runs `--verify-only` first: `wire_equal`, else
   `wire_equal_deterministic`, else `semantic_equal` (re-parse plus a
   deterministic-serialization fixpoint and identical text format — the C++
   `DebugString` check). Only verified cells are timed, with warmup and a
   settling GC outside the window. Output is one JSON object on stdout;
   exit 2 means verification failed (the driver fails the run), exit 3
   means structurally unsupported (recorded as `not_run`).

Verify-method notes (every cell's method is recorded): vtprotobuf
serializes maps in Go iteration order, so the map-bearing payloads
(`grpc-testing/request`, via nested Orca maps) flap between `wire_equal`
and `wire_equal_deterministic` run to run in the vt peer while the
pass/fail verdict stays stable (same as the upb peer's map cells);
hyperpb re-encodes through the generic reflection marshaller, which emits
declaration order rather than field-number order, so the
out-of-order-declared `google-messages` payloads stably land on
`wire_equal_deterministic` there. `otlp/metrics` and the xds payloads land
on `semantic_equal` in all three Go peers (field order differs from the
Python-generated bytes); the std and vt parses there are `proto.Equal`
with byte-identical re-serialization, so the method reflects the payload,
not a codec defect.

## Build

Normally built via the driver (`scripts/xlang-codec.sh --build-only
--peers go,go_vt,go_hyperpb`), which sets up the pinned `protoc` first.
Direct rebuild:

```sh
make -C bench/xlang/go   # PROTOC= defaults to target/pinned-protoc-build/protoc
```

Per-binary status lands in `target/xlang-codec/work/go-build.tsv`
(`<peer> <corpus> ok|reason`) for the driver's `not_run` reporting.

## Cell IDs

`xlang.<go|go_vt|go_hyperpb>.<corpus>.<short>.<tier>.<encode|decode>`,
`kind` `xlang_encode`/`xlang_decode`, reported in devloop/1 JSON with
per-op wall medians. Instructions are measured on Linux with `perf` (N/2N
differential like every peer); allocations are `not_run` (the Go heap is
not instrumented).
