# Public adoption corpora

This standalone consumer qualifies the SB-26 schemas and complete read walks.
The [matrix contract](../ADOPTION.md) defines the later timing workloads.
Every fixture is synthetic. There are no private schemas or captured payloads.

Build prerequisites are Rust 1.88+, an ordinary `protoc` compatible with
prost-build 0.14, and the existing pbrs source tree. This consumer does not
require Google's Rust/upb compiler or the repository's pinned conformance
checkout. Its reviewed manifest reuses pbrs, prost/prost-build 0.14 and
serde_json from the established benchmark graphs; the lockfile resolves prost
0.14.4. Shipping manifests and default graphs are unchanged.

```sh
# Reproduce schemas and generated fixture/field-walk source (Python + rustfmt).
python3 bench/devloop/adoption/generate.py
python3 bench/devloop/adoption/generate.py --check

# Compile both generators' actual messages, including twenty options files.
cargo test --locked --manifest-path bench/devloop/adoption/Cargo.toml
cargo clippy --locked --manifest-path bench/devloop/adoption/Cargo.toml \
  --all-targets -- -D warnings

# Run all equality/read checks and emit the deterministic inventory JSON.
cargo run --locked --manifest-path bench/devloop/adoption/Cargo.toml \
  --bin inventory

# Qualify all fresh/decode/read-all/fully-read-clone cell implementations.
cargo run --locked --manifest-path bench/devloop/adoption/Cargo.toml \
  --bin codec-inventory
```

Use an external `CARGO_TARGET_DIR` when building this excluded consumer.
Generated pbrs/prost messages remain in Cargo's output directory. The fixture
script generates the wide schema, its two independent field walks and the
options schemas/tests; their checked-in outputs must only change through that
script. `--check` also verifies formatting using the installed rustfmt, whose
version is recorded in qualification evidence.

There are 64 inventory specimens: 48 recursive queries, three entity lists,
ten sparse presence patterns and three map sizes. Full equality is checked by
decoding pbrs's wire output into the independent generated prost type. Two
separate field walks must also agree. Map byte order may differ; equality and
read checksums are independent of map iteration order. Every byte of strings
and bytes is consumed. Any values are also decoded as the generated public
Payload type and all their inner fields are consumed by both codecs.

The tests cover all sparse fields with explicitly present defaults, every
typed oneof's present default, exact query depths, all predicate kinds,
modified bytes in the last entity and changed/reordered map entries. Every
options-file consumer also cross-decodes all sixteen fields. Compiling and
testing these fixtures establishes semantic equivalence, not a performance
result. SB-26b still owns the registered codec/RPC measurements, cold first RPC,
codegen/check timings and P1-P5 baseline. See [evidence](evidence/qualification.json).

SB-26c adds 512 separately named codec cells in `workloads::CELLS`, generated
from the 64-specimen contract by the same fixture script. Native fresh
constructors populate generated messages directly. Each codec constructs and
encodes its own Any Payload, computing non-data wire size with its own size
API before allocating the exact-length data field. Both recipes produce the
previously qualified bytes/values. Preparation checks full fresh/decode/clone
equality and every read checksum; the additional test also executes every
cell. Fresh encoding of map messages produces four extra bytes in pbrs for
default-valued entries. Both lengths are retained, and decoded values agree.

The parent devloop harness uses these operations with its existing exact
allocator and N/2N instruction collector. Its broader comparator graph still
requires genuine pinned `protoc 35.1` with Google's Rust generator. On macOS,
`/opt/homebrew/bin/bash scripts/devloop-linux.sh` can build that toolchain and
run the collector in Linux; use Bash 4+ for the wrapper's empty arrays under
`set -u`. Missing instruction counters remain unavailable measurements.
When the existing pinned source checkout is present, both fixture generators
receive its well-known-type include directory; this also supports protoc
built from source without an installed include directory. The standalone
consumer still works with an ordinary installed compiler and no checkout.

After building the parent release binary, `bash
bench/devloop/adoption/measure.sh` runs all 512 cells at the frozen initial
N=16 and three repeats. Set `PBRS_ADOPTION_DEVLOOP_BIN` for a different
target directory and `PBRS_ADOPTION_OUT` for the eight 64-cell reports. The
runner uses the parent's collector, with its unchanged preparation, warmup,
allocator, missing-counter states and N/2N protocol. On Linux use the pinned
image/toolchain recorded with the result. The reports include every variant;
the runner stops on a failed cell and preserves completed earlier reports.
The [codec inventory](evidence/codec-inventory.json) and
[qualification](evidence/codec-qualification.json) retain the operation checks;
the allocation/instruction matrix remains separate SB-26c evidence.
The [combined codec baseline](evidence/codec-baseline.json) and
[measurement record](evidence/codec-measurement.json) now retain all 512
measured cells, eight original reports and 256 comparisons. Run
`python3 bench/devloop/adoption/check-evidence.py` to audit report hashes,
coverage, raw/combined row equality, ratios and target counts without changing
the artifacts. P1/P2 remain unmet in this instrumented dev-loop diagnostic;
The complete RPC matrix, startup and codegen measurement remain open.

SB-26d's `rpc-inventory` binary qualifies the same 64 specimens for unary
and four-message server streaming before transport timing. Both request
templates are decoded from the same prost wire, completely read and cloned
per RPC; fresh construction remains a separate codec operation. The oracle
checks full decoded equality, complete reads and request/response byte counts,
including response clones. Run it with the ordinary compiler and retain JSON:

```sh
cargo run --locked --manifest-path bench/devloop/adoption/Cargo.toml \
  --bin rpc-inventory
```

The [RPC inventory](evidence/rpc-inventory.json) exposes three blocked map
specimens: pbrs emits four extra bytes for default-valued map entries, even
after parsing the common prost wire. All 64 specimens have equal decoded
values and read checksums; 61 have equal encoded lengths. Unequal lengths
must fail the RPC timing preflight. This qualification is not a transport
measurement and does not close SB-26d.

The parent now registers 512 `rpc.adoption` cells. Build its existing release
binary with pinned protoc 35.1, then run the complete qualification/collector:

```sh
python3 bench/devloop/adoption/measure-rpc.py \
  --binary target/devloop/release/devloop --out target/adoption-rpc \
  --qualify-only
python3 bench/devloop/adoption/measure-rpc.py \
  --binary target/devloop/release/devloop --out target/adoption-rpc
```

The runner checks that the registry covers all 512 IDs, exercises actual
network replies for the 488 equal-wire cells and verifies that all 24 map
cells reject timing. It saves progress before measurements, retains eight
61-cell reports and a combined baseline, and compares every available metric
against tonic/prost without discarding losses. Missing instruction/syscall
tools stay visible in the existing report format. Source/binary/tool pins
and hashes accompany the results. The defaults are N=16 and three repeats;
explicit iteration/repeat overrides are recorded, not silently substituted.
Use `--jobs 3` on a host with sufficient resources to collect independent
reports concurrently; the record retains this setting. Paired N/2N child runs
within each cell remain sequential. Syscall/futex rows include setup and
warmup under the existing collector and remain diagnostics.

Audit a completed output directory without collecting new measurements:

```sh
python3 bench/devloop/adoption/check-rpc-evidence.py target/adoption-rpc
```

The audit checks all 512 qualification states, 488 raw/combined measured rows,
366 comparisons, report hashes and source bindings. It accepts the collector's
flat layout and the checked-in `rpc-raw/` layout. Evidence-only commits may
advance main during collection; every report's commit must retain the pinned
runtime source. A source change invalidates that combination of reports.
The inventory is loaded from the baseline's source commit so later codec
corrections leave historical qualification auditable.

The final [RPC baseline](evidence/rpc-baseline.json) and
[measurement record](evidence/rpc-measurement.json) retain 488 measured cells,
24 blocked map cells, eight raw reports and all 366 comparisons. Runtime
source is `48dff5ec4f13070c9c0930305b3d339273849130`; N=16/2N=32, three repeats,
100-RPC warmup, two Tokio workers and three collectors were used. Both stacks
enable TCP_NODELAY and leave TCP keepalive unset. RPC allocation metrics are
medians of exact per-run counts. Audit the checked-in artifacts with:

```sh
python3 bench/devloop/adoption/check-rpc-evidence.py \
  bench/devloop/adoption/evidence
```

The eligible RPC floor is unmet: 286/366 instruction and 217/366 allocation
losses against tonic/prost. Neither native profile meets either proposed P3
margin on any of its 122 measured pairs. The
[evidence summary](../../../docs/evidence/sb-26.md) retains every corpus's
cost ranges and limits. SB-26d remains blocked on the map equal-wire policy;
these results do not close the parent program or establish native latency.
