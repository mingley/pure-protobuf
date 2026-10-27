# Required fields after merging

The decoder now accepts required child fields split across separate occurrences:
`child { a: 1 } child { b: 2 }` becomes one initialized child with both fields.
Initialization is checked on the final message after merging. Invalid wire data
still fails. Regenerate affected consumer bindings to get the fix.

**Adopted:** kernel `af00f348`, benchmark `eac85b04`, test isolation `b20c413a`.
The benchmark adds deterministic
zero/noise payloads and verifies full bodies for Prost TestService. These changes
do not establish production readiness or a performance win.

## Checks

| Check | Result |
|---|---:|
| Generated consumers, including parallel/imported types | 33 passed |
| Root core library | 113 passed |
| Runtime integration / MiniTable-upb integration | 26 / 18 passed |
| Official conformance, required twice and recommended once | Each 5,631 binary/JSON + 909 text passed |
| Thirteen bundled bindings | 11 unchanged; 2 proto2 outputs regenerated |
| Selected strict Clippy, formatting and regeneration | Passed |
| Root all-feature workspace compilation (`cargo check`) | Passed; no full workspace test run |
| Root AddressSanitizer, leak detection disabled | 190 passed across 5 executables |
| Benchmark unit tests | 5 passed |
| Debug benchmark endpoint checks | 24 positive captures / 48 RPCs; 8 expected rejections |

Root integration also records a failed LeakSanitizer attempt: all 113 core tests
pass, then LeakSanitizer exits fatally and prints its ptrace limitation. The exact
fatal cause is unavailable; ptracing was not independently established. That
process supplies no leak-check pass. A separate AddressSanitizer-only run, with
leak detection disabled, passes 190 checks across five actual executables: 113
core, 26 runtime, 18 MiniTable-upb and 33 generated-consumer cases.

The official v35.1 checks cover through Edition 2023 using the newly built
candidate. Detecting controls cover singular and oneof merges, ancestors,
last-wins maps, groups, MessageSet extensions, cold fields, 65 required fields,
empty enforcing merges and shared input backing. The original six merge failures
and later review-gap failures remain in the archive.

Required-reachable validation now builds a temporary message tree and can
allocate. Required-free generated paths retain their bytes. Python protobuf
6.33.5/upb agrees with ordinary initialization and map controls; it cannot load
the 65-required-field type and differs from C++ v35.1 on the MessageSet merge.
Those reference limitations and the corrected sharing-fixture threshold remain
in the record. A later Root check exposed a test-harness collision: concurrent
single/parallel consumers used the same package and executable path. Distinct
package names and explicit 30/3 child-summary checks now prevent a successful
process exit from masking the wrong consumer. The original receipt is preserved.

## Experiments kept separate

| Candidate | Observation | Decision |
|---|---|---|
| Wake proxy `9235cb82` | 100 focused checks and 360,000 RPCs pass; CPU repeats are mixed | Not adopted |
| Encode scratch `06ec9f23` | 738 library checks; four 1 MiB Prost scratch requests fall from 4.00 MiB to 1.00 MiB | Not adopted; endpoint matrix unrun |

The scratch measurement is one allocation site, not total endpoint savings.
Custom contiguous codecs add a temporary allocation and copy on every message.
The idle encode allowance is separate from the existing decode allowance; neither
is a process RSS cap. A documented successor would make append-only encoding an
explicit capability and retain the old path for custom contiguous codecs. That
design is unimplemented and still needs measurements.

The wake CPU campaign uses counters-off shipping builds, 48 captures and balanced
source/control order. Unary native-server raw CPU is 77→103 and 62→62 µs/call;
bidi is 587→191 and 215→203. N/2N slopes and tonic controls vary greatly. No
consistent CPU benefit is established. A later eight-capture affinity diagnostic
fails at its first listener before any RPCs with `Operation not permitted`.
This session also rejects network permission escalation. The bytes matrix remains
unbuilt and unrun; planned counts are not results.

## Resource days

Both `933bf877` 24-hour attempts are interrupted after an environment restart.
The deadline run last records 43,781 cycles and 12.35 measured hours; the general
run records 13,673 cancellation cycles without a final measured duration. Their
workers are absent, no original terminal result survives, and the exit cause is
unknown. No application failure is established. Neither attempt passes 24 hours;
fresh same-source retry grants are prepared but have not launched.

The [earlier runtime checks](../grpc-ownership-20261024/README.md), including its
3,840 workspace and 961 sanitizer checks, belong to that recorded source. They
do not qualify this kernel change. DynamicMessage and MiniTable singular required-field merging need separate
coverage. Full exact-source feature/platform, endpoint performance and
sustained-resource qualification remain open.

## Records and reproduction

[Results and source pins](results.json) · [Manifest](manifest.json) ·
[Raw sources, commands, controls and captures](raw.tar.gz)

Run `python3 check.py` to verify all member and publication hashes without
extracting files. The archive retains source snapshots, patches, command receipts,
reference inputs, failed controls, captures and hashes for omitted ELF bytes.
Local absolute paths in receipts describe the original execution environment.
The archive does not include toolchain executables or replace their provenance.

Three Git bundles retain the adopted code and both unadopted experiments. With
base `ee5b345b` already fetched, an experiment can be inspected in a detached
worktree:

```sh
tar -xzf raw.tar.gz qualification/architecture-c15-wake-experiment.bundle
git fetch qualification/architecture-c15-wake-experiment.bundle HEAD
git worktree add --detach ../wake-experiment 9235cb825947a39527b860d1dac2333f4b94642f
```

The adopted bundle is `qualification/architecture-c15-adopted-source.bundle`,
ending at `b20c413a`; it retains the benchmark, kernel fix and test-isolation
commits. It excludes later documentation.

The bytes bundle is `qualification/architecture-c15-bytes-experiment.bundle`;
its candidate is `06ec9f23d1f1a40af4c6b4b4e1def2f4064dfcb2`. Both experiments
need fresh endpoint measurements before an adoption decision.
