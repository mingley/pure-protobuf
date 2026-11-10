# Run benchmarks

[Results](../benchmarks.md) · [Measurement rules](../benchmark-contract.md)

Run commands from the repository root with Rust, Cargo, Python 3 and `protoc`
installed. Keep results in a new directory so older runs remain reproducible.

## gRPC client and server

For a quick local comparison using only the Rust harness:

```sh
bench_output=$(mktemp -d)
cargo run --locked --release --manifest-path rpc-bench/Cargo.toml -- \
  smoke --quick --output "$bench_output/quick.json"
```

This runs both stacks in one process and prints its tables and gate results.
It saves the report even when a fairness or performance gate returns a failure.
Short smoke timings are diagnostics; inspect the failing rows before drawing a
performance conclusion.
Use separate endpoint processes for CPU attribution. The
[official QPS runner](../../scripts/grpc-qps-interop.sh) additionally needs a
Go or C++ driver and the pinned reference tools described in the RPC guide.

For separate client/server allocation measurements, start with a clean
checkout and freeze its executable:

```sh
source_commit=$(git rev-parse HEAD)
bench_output=$(mktemp -d)
python3 scripts/build-rpc-bench.py --source "$source_commit" --output "$bench_output/build"
python3 scripts/grpc-load-smoke.py \
  --binary "$bench_output/build/rpc-bench" \
  --build-record "$bench_output/build/build.json" \
  --allocation-counts --payloads=1024 --rpc-count=20 --duration=60 \
  --output "$bench_output/results"
```

Read `report.json` first, then the failing cell's `run.json` and endpoint logs.
The harness checks terminal RPC counts and records the source and binary hashes.
This example runs 320 cells: four client profiles, four server profiles, five
call shapes, plaintext/TLS and identity/gzip. Each cell attempts 20 RPCs with a
60-second load-generation limit; the full run can take several minutes. An
interrupted run keeps `manifest.json` and completed cell files; `report.json`
is written when the matrix finishes.

The builder enables allocation counters by default. Omitting the runner's
`--allocation-counts` flag does not remove instrumentation from that binary.
For CPU and latency diagnostics, build and record a separate binary with the
benchmark package's Cargo allocation-count feature disabled:

```sh
source_commit=$(git rev-parse HEAD)
bench_output=$(mktemp -d)
python3 scripts/build-rpc-bench.py --source "$source_commit" \
  --no-allocation-counts --output "$bench_output/build"
python3 scripts/grpc-load-smoke.py \
  --binary "$bench_output/build/rpc-bench" \
  --build-record "$bench_output/build/build.json" \
  --payloads=1024 --rpc-count=20 --duration=60 \
  --output "$bench_output/results"
```

The build record checks the Cargo command and the benchmark package's resolved
artifact features as well as source, binary and log hashes. The runner retains
and hashes its build verifier when using a record. A verified plain build
cannot be used for allocator snapshots. Use an external or ignored output
directory: for a verified run inside the measured checkout, all known output
files must be Git-ignored. The runner checks them before starting cells and
revalidates the source afterwards; unexpected tool files or concurrent changes
can still fail the run. Verified source checkouts must not mark tracked files
`assume-unchanged` or `skip-worktree`; this also excludes typical sparse
checkouts. Use a full checkout with those index flags cleared.

`source.patch` records tracked edits in the measured checkout at capture
start; `harness.patch` records tracked edits in the driver checkout. Their
hashes appear in the report. Each is the tracked worktree's net diff from
HEAD; it does not preserve index staging separately, untracked files or dirty
submodule contents. Git index flags and configuration can hide some tracked
changes, so these are not byte-for-byte checkout attestations. `dirty` and
`harness_dirty` request untracked files at capture start. Without a
build record, `source_verified` is false and `source_unchanged` does not
verify source drift.

Cargo's package feature list records its resolution, not every effect on the
executable: custom Rust flags or compiler wrappers can alter compilation, and
dependency feature unification is separate. Inspect those inputs and the
dependency graph before comparison; this record alone does not prove the
compiled executable has no allocation instrumentation.

The no-counter source-pinned builder must exist in the source checkout being
built. Older checkouts with the legacy builder can only produce its instrumented
record; the current driver can still validate those legacy records. Building an
older uninstrumented baseline needs a separate verified build workflow.

The [RPC harness guide](../../rpc-bench/README.md) covers all call shapes,
compression, TLS, open-loop arrivals and independent peers. For noise payloads,
both endpoints need matching `--payload-pattern=noise`, `--payload-seed`,
`--req-bytes` and `--resp-bytes`; that mode currently requires Prost messages.
The [encode evidence capsule](../evidence/grpc-encode-reuse-20261026/README.md)
retains the exact drivers and commands for the one/four/64-message comparisons.
The [server CPU follow-up](../evidence/performance-fixes-20261027/README.md)
adds separate CPU captures for wake coalescing.

## Development-loop comparisons

The [development-loop harness](../../bench/devloop/README.md) combines codec,
RPC and generator comparisons. Its RPC reports identify the canonical prepared
input and its encoded length. Compare those fields across processes, while
keeping semantic and wire-length checks enabled. Map serialization order can
vary without changing the prepared message.

For short and full server-stream bursts, use the [opt-in burst
diagnostic](../../bench/devloop/README.md#opt-in-server-stream-burst-diagnostic).
The [Query allocation diagnostic](../../bench/devloop/QUERY-ALLOC.md) separates
request cloning, decoding, field reads and response encoding.

Use a fresh inventory when collecting the standalone tonic transport matrix.
The collector derives qualified and blocked counts from that inventory; an old
488-cell selection does not describe the current 512 qualified states. Legacy
records remain readable, but missing input metadata cannot qualify a new
capture. See the [accounting follow-up](../evidence/readiness-followup-20261102/README.md#benchmark-inputs-and-qualification)
for the detecting failure and repair scope.

## Message codecs

For owned and borrowed byte buffers, the [mixed-byte ownership diagnostic](../../bench/devloop/README.md#opt-in-mixed-byte-ownership-diagnostic)
compares construction and serialization allocations for pbrs and Prost.
Its measured scope includes the source buffers and message drop.

Use the [codec survey](../../tonic-bench/README.md) for encode, decode and
field-access comparisons. Keep parse-only, parse-and-read, owned messages and
borrowed views separate. Include first encode and unchanged repeated encode;
they exercise different work.

For a quick depth-64 read-all probe using the pinned Prost harness:

```sh
cargo run --locked --release --manifest-path tests/proof/codec_benchmark/Cargo.toml \
  -- nested_64 pbrs read_all 10000 3
cargo run --locked --release --manifest-path tests/proof/codec_benchmark/Cargo.toml \
  --features owned-messages -- nested_64 pbrs read_all 10000 3
cargo run --locked --release --manifest-path tests/proof/codec_benchmark/Cargo.toml \
  --features owned-messages -- nested_64 prost read_all 10000 3
```

Use the [codec harness guide](../../tests/proof/codec_benchmark/README.md) for
complete paired runs and separate allocation builds. The
[storage guide](../guides/codegen.md#owned-nested-message-fields) explains how
to choose the application's profile. Keep parsing, full reads, partial reads
and encoding separate; a quick single-row probe cannot establish a ranking.

## Code generation and consumer builds

```sh
./scripts/codegen-bench.sh --case small
```

The [codegen harness](../../bench/codegen/README.md) explains comparator pins,
generation time, generated bytes, cold consumer builds and incremental builds.
Larger corpora and independent generators need the tools listed there.

## Check retained measurements

These commands verify the archived source hashes and recompute the retained
results; they do not run a new benchmark:

```sh
python3 docs/evidence/balanced-authority-storage-20261106/check.py
python3 docs/evidence/hosted-devloop-10d94697-20261106/check.py --negative-controls
python3 docs/evidence/hosted-devloop-20261106/check.py --negative-controls
python3 docs/evidence/rejected-medium-core-utf8-20261106/check.py
python3 docs/evidence/map-equality-20261105/check.py
python3 docs/evidence/byte-ownership-20261105/check.py
```

Each record's README explains its measured scope and how to collect new data.

## Read a result correctly

- Check the source, codec, payload, stream length and concurrency before comparing rows.
- Keep client and server CPU separate, and report failures alongside successful calls.
- Requested allocation bytes measure allocator traffic; RSS measures retained process memory.
- Keep shared-host diagnostics separate from controlled-host qualification.
- Preserve raw captures, unsuccessful attempts and losing workloads.
