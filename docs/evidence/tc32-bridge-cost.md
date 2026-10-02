# TC32b: actual bridge cost collectors

Date: 2026-10-02. Source: `d9a7c323586e11e649c40e605ff13b45b2a8b013`
(commits `5a263139`, `e946b5fd`, `d9a7c323`) on integrated SB32/TC32a
base `1d0f56bdf696f801d9c8ccad286a253a871213e6`.

The parent devloop now registers TC32a's 336 actual-API bridge cells behind
an optional `bridge` feature. The ordinary default graph retains 153 active
packages and all 1634 existing registry lines. The enabled graph has 154
packages and 1970 lines; filtering its 336 appended bridge lines reproduces
the default registry byte for byte. The lock adds only the existing path
package `protobuf-tonic` and the adoption-to-adapter and tonic-to-existing-flate2
edges. Every original package version, source and checksum is unchanged.
Shipping dependencies and default features were not edited.

`run-cell codec.adoption.bridge.*` invokes `BridgeCase` and rejects map timing
before warmup or allocator arming. Both N and 2N prepare the same completely
read source pair and run 100 warmup operations. The existing counting allocator
then covers each real API conversion, the optional complete target read, and
the owned target's drop. Source preparation, full equality/round-trip oracles,
qualification JSON, and output printing are outside that allocation window.
Instruction counts include the defined dispatch/loop/drop work and use the
existing N/2N process-counter subtraction; they are absolute operation costs,
with no prost-minus-native estimate. Bridge children require nonempty matching
actual-source fingerprints for differential, repeated and replay reports.

The new stdlib-only capture driver retains every uninstrumented allocator
child and every Callgrind N/2N child/graph, with three repeats and common
`--prepare-iters 2N --warmup 100`. Graphs are losslessly compressed with both
original and compressed SHA256 pins. Child qualifications must match the
preflight and SHA-pinned TC32a complete-work oracle, including actual source
wire identity. The independent auditor rebuilds every allocation/byte value and
instruction delta from raw records, requires exact allocation values across
all six uninstrumented N/2N/repeat children, rejects negative/missing/ambiguous
instruction counters, and audits complete coverage. Unavailable instructions
remain `not_run`. A selected calibration subset cannot claim full coverage.

Two captures of the same source/binary can be compared with the auditor.
It retains every original directional >1% instruction regression and reports
signed and absolute variation. Negative instruction variation greater than
1% also leaves the replay stability guard false. Allocation replay requires
exact equality. Missing metrics cannot produce a replay pass. Original full
frozen control campaigns remain separately required, with their historical
failures unchanged; these collectors do not certify those campaigns.

Final source checks pass: 20 bridge-enabled tests (including the unchanged
488 eligible/24 blocked SB32 network matrix), 17 default tests, strict
all-target bridge Clippy, 15 collector corruption/provenance tests, generated
output checking and scoped formatting. The final Python-only strengthening
commits leave every tracked Rust source, manifest and lock byte-identical to
the first gated implementation commit. Actual parent inventory checks pass
all 336 semantic cells and all retained complete-work oracles; 324 wire guards
pass, while all 12 map costs remain blocked. No source ordering or map defaults
were patched inside the API.

The enabled debug CLI SHA256 is
`247aa72e67e23332d0832eb9a0fc363a5eb5dea9252af3e21eb707ac54e36ccd`.
The default registry SHA256 is
`33b529f76973efd5478fe4c3f5a40929a6c2ce564eb25cb22b76f9b31b4adb90`.
[Retained correctness artifacts](tc32b-collector-20261002/artifact-sha256.json)
include exact commands, both feature proofs, all 153 old package pins,
source/tool hashes and original SB32 cache-output provenance. The shared debug
cache remained below 2 GiB and free space above 2 GiB; jobs were limited to one.
Debug binaries and synthetic corruption fixtures are correctness evidence.

No release build or performance capture has run. Allocations, allocated bytes,
differential instructions and unchanged-binary replay remain `not_run`.
TC32b and the parent TC32 remain open until actual cost acceptance is qualified.

## Frozen capture interface

A coordinator-approved quiet lease and a clean shared source-pinned release
build with `--features bridge` are required before running:

```text
python3 scripts/measure-tc32-bridges.py --binary /absolute/release/devloop \
  --build-pin work/shared-release/build-pin.json \
  --original-registry work/shared-release/default-registry.txt \
  --out work/tc32-cost/replay-one --iters 16
python3 scripts/audit-tc32-bridges.py work/tc32-cost/replay-one
```

The shared build-pin JSON must supply `source_commit`, `binary_sha256`,
`profile: "release"`, `features` containing `bridge`, `protobuf_source_commit`,
`source_sha256`, and `tools`. Source hashes must cover the 41 required paths
listed in `audit-tc32-bridges.py`, including every adoption schema, generators,
actual API and collector source. Each compiler/tool entry supplies `path`,
`version`, and `sha256`; rustc, cargo and genuine protoc are required. Further
SB32 build/schema provenance can be retained in the same sidecar. The driver
checks clean source/upstream trees, current source/tool hashes, and binary
hashes before/after every child. Use an ignored `work/` output directory so
capture artifacts do not change the clean source tree.

Run a second output directory with the same immutable source/binary and
arguments, then use `--baseline work/tc32-cost/replay-one` on the second audit.
`--cells` is reserved for an explicitly bounded calibration, whose report
retains all 336 preflight states and labels full eligible coverage false.
The 12 blocked map cells cannot be selected. Full raw storage and the 2 GiB
free-space reserve must fit before scheduling the campaign; missing results
stay visible and cannot close numeric acceptance.
