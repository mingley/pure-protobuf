# SB-32 measurement driver and independent audit

Source `61e6f9f1` adds only three benchmark support scripts. The frozen
gRPC workload, code generation, old collectors, controls, thresholds, main
registry, manifests and lockfiles are unchanged. This is collector
implementation evidence: **no release build, RPC qualification run or
performance capture was executed by this leaf**. The 512-cell numeric
matrix and TC-29/TC-30 performance qualification remain open.

## Capture contract

`bench/devloop/measure-tonic-transport.py` requires the single coordinated
release binary and its build provenance sidecar. It checks the binary hash
against that sidecar and verifies every frozen compiled source hash before
measurement. The subset includes runtime, transport, generator, schema,
corpus and path dependencies. Pinned Google Any/descriptor schema bytes are
also archived and checked. Source, schema and binary hashes are rechecked
between batches and at completion; resume requires the same immutable
source checkout, binary, tool paths, inventory and build record.

The sidecar's normalized required fields are `binary_sha256`,
`source_sha256` (repository-relative compiled inputs), `schema_sha256`
(including `third_party/protobuf/src/google/protobuf/{any,descriptor}.proto`),
`build_argv`, `features`, `profile: "release"` and `tools`. The root-owned
shared build record additionally retains the clean commit/tree,
compiler/protoc actual executable pins, profile/environment, generated
binding hashes, dependency graph/fingerprints, UTC times, build exit and raw
stdout/stderr. The driver archives that complete record byte for byte;
checking a live source checkout alone does not qualify a release binary.

Before collection, every one of the 512 new IDs executes a child preflight.
All 24 original map cells must fail equal-wire preflight, with their actual
stdout/stderr/argv/exits retained. Eligible cells must match the source-pinned
full decoded value/read/byte/count/fingerprint and actual accepted-socket
oracles. Both the new 512-ID registry and original 512 typed RPC IDs are
checked. The archive records blocked/qualified/captured states separately.

The driver runs two independent full 122-cell unchanged reference/reference
baseline replays, then all eight 61-cell eligible profile/shape batches.
Every batch invokes the **existing** devloop parent collector at N16,
2N32 and three repeats, with common preparation 32, default warmup 100,
two-worker child runtimes and default message caps. It preserves the
existing perf-first/Callgrind-fallback choice. Missing or failed counters
remain unavailable or stop that attempt; there is no hidden tool fallback
that turns a failure into a win.

Transparent tool wrappers retain byte-exact stdout/stderr, actual and
collector argv, UTC times and process exits **before forwarding output to
the parent's parser**. Callgrind changes only its artifact destination and
archives its complete output file with compressed and uncompressed hashes.
Every strace invocation retains its complete summary, including failed
runs. Failed collector attempts and interruptions remain indexed rather
than being replaced by a retry; unspawnable tools have an explicit spawn
disposition instead of an invented process exit.

Strace rows are **N16-only whole-process counts divided by 16**. They include
preparation, network oracle, warmup 100 and teardown. They are not hot-only
or differential syscall costs. Futex counts include wakes and errors and
do not count lock acquisitions.

## Independent reconstruction

`bench/devloop/check-tonic-transport-evidence.py` independently checks
source/build/schema hashes, the frozen 64-specimen inventory, all 512
states, raw file/index coverage and the 12 reports. It binds each child
iteration marker to the actual captured `--iters` argv, verifies common
N/2N fingerprints and network oracles, and reconstructs exact allocator
medians, differential instruction counts and whole-process syscall/futex
medians from raw output. Callgrind's stderr count must agree with the
archived summary/totals file. Missing strace summaries cannot hide behind
an otherwise plausible report median.

The audit retains all 366 candidate/reference comparisons and losses,
per-metric availability and both directions of the original **2% RPC**
baseline replay gate. Original codec/20-control 1% scope remains separate
and unchanged. Audit exit success means the archive is internally
consistent; failed replay gates or unavailable metrics remain explicit
and do not become performance qualification.

After the shared source-pinned build and coordinated measurement lease:

```sh
python3 bench/devloop/measure-tonic-transport.py \
  --binary <shared-release-devloop> \
  --build-record <shared-release-build.json> \
  --inventory docs/evidence/sb-32/preflight-inventory.json \
  --out <new-owned-evidence-directory> --iters 16 --repeats 3 --jobs 1
python3 bench/devloop/check-tonic-transport-evidence.py \
  <owned-evidence-directory> --out <new-audit-summary.json>
```

`--qualify-only` stops after all 512 preflights; `--resume` continues the same
immutable capture and skips completed batches. The independent audit needs
a complete capture. No output archive or prior audit is silently overwritten.

## Synthetic correctness gates

Eight Python-only tests pass. They exercise byte-exact failed captures,
unspawnable tools, strace failure retention, Callgrind summary archives,
allocation/instruction/syscall reconstruction, missing counters remaining
`not_run`, lost strace rejection, altered fingerprints, rehashed counter
and N/2N marker corruption, complete 512-state coverage, blocked-map
tampering and visible baseline replay losses. The full-envelope fixture
uses simulated data and separately tested counter reconstruction; it is
not a gRPC benchmark. Python compilation and source whitespace checks pass.
Exact source hashes, command, UTC times and raw test output are retained in
[validation.json](sb-32-driver/validation.json).
