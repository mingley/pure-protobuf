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
The [codec inventory](evidence/codec-inventory.json) and
[qualification](evidence/codec-qualification.json) retain the operation checks;
the allocation/instruction matrix remains separate SB-26c evidence.
