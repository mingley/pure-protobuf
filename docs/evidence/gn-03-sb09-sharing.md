# GN03 SB09 shared-descriptor profile and measurement preparation

This record qualifies the benchmark wiring at
`dea20f7ae6ec5d33fe4d7cb5fe64004cf6263247`. The small and 100-message corpora
each have one **unqualified local diagnostic pair**. A later 1,000-message
historical-source retry also completed one unqualified pair. The first
1,000-message attempt remains preserved as a cache-cap failure: its shared
profile, linked binaries, and paired comparison were **not_run**. Qualified
performance acceptance remains open. The
[source-sharing record](gn-03-source-sharing.md) proves source-volume reduction
and functional behavior; it does not prove these remaining GN03 costs.

The benchmark driver maps `SB09_PBRS_SHARED_DESCRIPTOR_SET` through its existing
strict Boolean parser to `Config::shared_descriptor_set`. Unset/false preserves
ordinary generation. The report adds raw/resolved emission and runtime profile
provenance globally and per pbrs cell, with actual helper activation, path, size,
and hash. Missing or stale helper output is an error. An explicit false flag
retains its raw value but resolves to the ordinary profile. Nondefault shared or
lean profiles carry a diagnostic label and qualification reason. Native/tonic
stub runtime dependencies retain their existing default profile.

No generator registration, corpus/seed, consumer round-trip work, threshold,
registry pin, schema version, default options, or release settings changed. The
existing consumer includes the root registry, so the opt-in helper needs no
consumer workaround. Smaller generated source may produce equal linked binary
size if the linker removes unused metadata; that result will remain visible.

## Source gates

The unchanged baseline harness passed 62 tests with one optional pinned
generation test skipped. New profile contracts first failed with three missing
profile API errors. After the focused implementation, all 66 tests passed with
the same optional skip:

```sh
python3 -B -m unittest discover -s bench/codegen -p 'test_*.py'
rustfmt --check --edition 2024 bench/codegen/generator.rs
git diff --check
```

Rust formatting and whitespace checks passed. The tests cover ordinary/unset
and explicit false profiles, true/lean diagnostic profiles, raw inputs,
resolved JSON/text defaults, actual stub runtime selection, missing/stale helper
rejection, and a complete mocked generation/check/release/smoke pipeline. They
retain the original corpus hash assertions and verify unchanged generated-file
checks include the helper. Benchmark-specific Rust driver compilation and live
generation are **not_run** here; they occur at the common bootstrap for the
leased captures. This record does not present mocked phases as measured costs.

[gates.json](gn-03-sb09-profile-artifacts/gates.json) retains commands, status,
source hashes, and resource plan. The
[artifact directory](gn-03-sb09-profile-artifacts/artifact-sha256.json) hashes
the raw baseline/red/after logs, exit records, and prepared work-only coordinator.

## Paired capture plan

Use one frozen copied generator executable for both profiles. Bootstrap is
excluded from measured cells. The work-only coordinator reuses the existing
`prepare_corpus` and `measure_pbrs_cell` functions. It runs separate initially
nonexistent Cargo targets with jobs=1 and the unchanged clean/incremental check,
thin-LTO release build, and release smoke. Initial order is small off/on, 100
on/off, and 1,000 off/on. Equal-length directory suffixes avoid a path-length
bias in embedded source locations. One pair per corpus is a local diagnostic,
not statistical or independent-host qualification.

Provenance will include source/tool/generator/input/manifest/lock hashes, raw
phase logs, actual linked executable bytes/hashes, and resource telemetry. Each
completed target is reclaimed only after preserving its evidence and executable.
The original reused excluded bootstrap was about 691 MiB. It was later reclaimed
after preserving its evidence, as described below. The cache limit is 2 GiB with
at least 2 GiB global free space; budget failures terminate only the coordinator's
own timed process groups and are retained. Capture leases can be released after
each corpus pair. Original failed attempts and deferred cold-consumer status
remain in the earlier source qualification record.

GN03 stays open until actual SB09 cold-check and linked-binary results are
assessed. Equal, losing, failed, or incomplete cells cannot be replaced by the
source-size proof, hidden by an aggregate, or converted into a performance claim.

## Coordinator review correction

Before any live capture, root review found that the prepared work-only
coordinator reused its `binary` local when retaining the first linked consumer.
That would pass the first consumer's path, after its target was reclaimed, as
the generator for the second profile. The original prepared script and its
hash `031f6f252f6b27530ea1ddcf6557b190127cec9079d2c4cd2da2458427a5f5c2`
remain unchanged in the original artifact directory.

The [additive revision](gn-03-sb09-profile-artifacts/coordinator-v2/artifact-sha256.json)
uses separate `generator` and `linked_binary` bindings. A main/import guard
allows a focused orchestration test to run the actual two-profile loop with a
stub compiler. The test first failed because the second profile received the
consumer path, and now passes. It verifies that both profiles use the same
frozen generator path and hash, start with distinct nonexistent targets, and
retain each linked executable and its fingerprint hashes before cache removal.
The test's stub compiler produces no performance evidence. The revision retains
the red and passing logs, exit records, exact test/script sources, and all three
plan previews. Benchmark source, workloads, release settings, and thresholds
are unchanged; numeric costs remain **not_run** pending a quiet capture lease.

To replay the no-build orchestration test from a repository checkout, copy the
two revised scripts into its ignored `work/` directory and run:

```sh
cp docs/evidence/gn-03-sb09-profile-artifacts/coordinator-v2/{sb09-paired-screen.py,test_sb09_paired_screen.py} work/
python3 -B work/test_sb09_paired_screen.py
```

## First small-corpus diagnostic

At source `8b6cc270af49838b3cd4793141ca5b269a795202`, one default/shared
pair used the corrected coordinator and the same copied generator executable
(`718aef4131813c1c6fc449e33202588fc6bf48cc330bd28cfd33a96e056f524b`).
All 69 compiled-source hashes matched root main's inputs before capture. Both
profiles used the original seeded six-message corpus, identical consumer
manifest and lock, jobs=1, distinct initially nonexistent targets, and the
unchanged opt-level=3/thin-LTO/codegen-units=1 release profile.

| Observed cost | Default | Shared |
| --- | ---: | ---: |
| Generated source bytes | 135,821 | 129,486 |
| Cold check seconds | 14.264721 | 13.811340 |
| Incremental check seconds | 0.167422 | 0.165418 |
| Release build seconds | 58.404685 | 57.624617 |
| Linked executable bytes | 545,936 | 545,936 |

Generation, unchanged-generation verification, clean and incremental checks,
release build, and release smoke passed for both profiles. The shared helper was
active only with the opt-in. Both release smokes produced the original `1\n`
output. The executables have different hashes but equal size; there is **no
binary-size win** in this cell. The existing consumer does not read reflection
metadata, which the linker may remove. This does not qualify reflective-consumer
binary cost.

The pair is explicitly unqualified: it has one run per profile, a fixed order,
no independent reference peer, and incomplete corpus coverage. Root also
reported a 0.374-second SV09 Python compression/hash operation around
23:41:32.856 UTC during the default release build. A separate quiet-window
sidecar retains that exception and the observed log-file times; the original
phase durations were neither adjusted nor replaced. These values are
observations, not a statistical cold-check or release performance claim.

Sampled owned-cache use peaked at 1,064,169,472 bytes, and sampled global free
space stayed at or above 5,765,459,968 bytes. No resource-limit failure was
recorded. Each completed target was removed only after retaining its actual
linked executable and compiler fingerprint hashes. The excluded common
bootstrap remains reusable.

The [small-pair artifacts](gn-03-sb09-small-artifacts/artifact-sha256.json)
retain a 156-member raw archive with reports and phase/tool logs, actual corpus
inputs and generated/consumer sources, manifests/locks, source/schema capsules,
the common generator and both linked executables, fingerprints, resource
telemetry, and quiet-window notes. The verifier checks every member hash/size,
same paired inputs/source/tools/generator/manifest/lock, original release profile,
generated source totals, helper activation, raw GNU-time RSS and exit status,
smoke output, linked-executable hashes/sizes, and sampled resource bounds:

```sh
python3 -B docs/evidence/gn-03-sb09-small-artifacts/check-pair.py
```

At this stage, the 1,000-message pair and repeat/control qualification were
pending separate root capture leases. The subsequent attempts are retained
below; qualified performance acceptance remains open.

## First 100-message diagnostic

At source `aa6fb299ffd9d905714d2b0ce551269a5d9552f1`, the 100-message pair
reused the exact small campaign's common generator, bootstrap driver lock,
69 compiled-source pins, schema capsules, and actual tool binary hashes. Its
order was shared/default. Separate initially nonexistent targets used jobs=1
and the original release profile, consumer workload, seeded corpus, and paired
consumer manifest/lock. All seven phases passed for each profile, including the
unchanged-generation checks and release smoke.

| Observed cost | Default | Shared |
| --- | ---: | ---: |
| Generated source bytes | 2,313,461 | 1,947,481 |
| Cold check seconds | 18.427257 | 18.368341 |
| Incremental check seconds | 1.320513 | 1.267451 |
| Release build seconds | 73.367904 | 73.772731 |
| Linked executable bytes | 1,446,704 | 1,446,704 |

Shared generation reduced source bytes by 15.82%. Its observed cold check was
0.32% lower, but its release build was **0.55% slower** and its executable had
**equal size** with a different hash. All raw timing and RSS metrics remain in
the reports. These observations do not establish a compile-time or binary-size
improvement. As with the small cell, the frozen consumer does not call
reflection; a separately designed metadata-retaining consumer would be an
additional diagnostic rather than a replacement for these controls.

The pair ran from 00:16:14.566103 to 00:19:21.911040 UTC on October 3, 2026.
Root held other compilers, Miri, captures, and large packaging operations, and
no quiet-window exception was reported. One run per profile, fixed order, no
independent reference peer, incomplete corpus coverage, and the nondefault
shared profile still leave this result explicitly **unqualified**.

Sampled owned-cache use peaked at 1,181,134,848 bytes and sampled global free
space remained at or above 6,293,164,032 bytes. No resource-limit failure was
recorded. Each completed target was removed after preserving its linked
executable and the original coordinator's fingerprint SHA256 inventory. The
coordinator records fingerprint digests, not the fingerprint file contents.

The [100-message artifacts](gn-03-sb09-100-artifacts/artifact-sha256.json) retain
168 raw archive members with the same full source/input/tool/report/executable
scope as the small proof and a separate quiet-window note. The original small
pair and its overlap note remain byte identical. Verify the 100-message archive
without running any compiler or archived executable:

```sh
python3 -B docs/evidence/gn-03-sb09-100-artifacts/check-pair.py
```

At this stage, the 1,000-message pair was pending. Its failure and later retry
are retained below. Repeated/control qualification and any separate
reflective-consumer diagnostic remain pending. GN03 remains open.

## Failed first 1,000-message attempt

Root launched the unchanged coordinator directly at source
`7ec9117d246dc91502520b6349d687d6136cc506`, with the exact same common
generator, source/schema/tool pins, and jobs=1 controls. The default attempt ran
from 00:31:20.043366 to 00:32:55.653432 UTC on October 3, 2026. Root collected
the exit-1 result and explicitly released the quiet/LTO lease at 00:37:17 UTC.
No quiet-window exception was reported.

Consumer lock generation, initial generation, unchanged-generation verification,
and default cold check passed. Output was 35,782,708 bytes in 21 Rust files.
Cold check took 80.580565 seconds, with recorded peak RSS 5,608,468,480 bytes.
This is one successful default phase, not a completed paired measurement.

During incremental check, the resource watchdog sampled owned-cache use of
**2,153,570,304 bytes** at 00:32:55.519467 UTC. That exceeded the unchanged
2,147,483,648-byte cap by 6,086,656 bytes. Sampled global free space remained
5,077,155,840 bytes, above its separate 2 GiB minimum. The watchdog's own timed
process-group termination left the incremental stderr without the GNU-time
RSS/exit trailer. The unchanged parser consequently reported
`missing Linux maximum resident set size in time log`. No incremental duration,
RSS, or child exit result is invented from the interrupted phase.

The original default report remains `error`, and the pair remains `incomplete`.
Default release build, smoke, and linked executable were **not_run**; the shared
profile never started. There is no binary or paired comparison result. The
failed target was reclaimed after the frozen coordinator retained its original
fingerprint SHA256 inventory; the inventory contains digests rather than the
original fingerprint file contents.

The [failed-attempt artifacts](gn-03-sb09-1000-failed-artifacts/artifact-sha256.json)
retain 154 archive members, including every successful/interrupted child log,
argv-bearing report, actual inputs and generated/consumer sources, locks,
source/schema capsules, common generator, telemetry, and quiet-window notes.
Root's exact tool-returned start/completion chunks are retained as a supplemental
record. Separate coordinator stdout/stderr streams were not captured at launch;
the record does not reconstruct them or invent missing process metadata.

```sh
python3 -B docs/evidence/gn-03-sb09-1000-failed-artifacts/check-failed-attempt.py
```

The verifier checks all member hashes/sizes, unchanged source/generator/profile,
successful raw child exits/RSS, exact cache-cap event, missing interrupted
trailer, and absent shared/release results. Any later retry must retain this
failure, use an additive namespace and the same frozen generator, and obtain a
new quiet lease. These artifacts do not alter the cap or benchmark controls.
GN03 remains open.

## Excluded bootstrap cache reclamation after the failure

After the failed-attempt proof was retained, root authorized reclaiming only the
completed excluded bootstrap cache. Its literal GN03 path is `target/base`,
resolving to `/workspace/scratch/work/gn11/target/base`; the separate
`target/integration-consumers` symlink resolves to the same location. The copied
generator is outside that cache and remains unchanged, as do its record,
driver lock, source/tool pins, and all small/100/failed-1,000 measurement records.
No benchmark phase, cap, or consumer was changed, and no retry was started.

The [reclamation artifacts](gn-03-bootstrap-reclaim-artifacts/artifact-sha256.json)
record all 1,772 file-path hashes and retain the actual contents of 652 compiler
fingerprint/dependency files. Two process checks, including one immediately
before deletion, found no cache-path ownership in accessible executables, cwd,
argv, Cargo target environments, open descriptors, or mappings. All compiler
processes were inspectable. Root-owned `dockerd` and `containerd` exposed their
names and arguments but denied several other `/proc` reads; the exact coverage
limits are retained rather than presented as complete system-wide inspection.

| Before reclamation | Bytes |
| --- | ---: |
| Logical bytes summed across file paths | 877,185,904 |
| Allocated bytes summed across file paths | 880,754,688 |
| Logical bytes after inode deduplication | 808,147,304 |
| Allocated bytes after inode deduplication | 812,244,992 |
| Allocated directory bytes | 1,347,584 |

There were 52 multiply linked paths. Observed global free space increased from
5,556,858,880 to 6,370,451,456 bytes, a difference of 813,592,576 bytes. That
matched unique-inode allocation plus directory allocation in this observation;
global filesystem deltas can also include unrelated concurrent activity. The
resolved directory was recreated empty, preserving both approved symlinks.

```sh
python3 -B docs/evidence/gn-03-bootstrap-reclaim-artifacts/check-reclamation.py
```

This verifier checks recorded inventory accounting, every retained content
hash, process-check results and limitations, and the frozen generator pin.
The unused cache is absent, so the remaining nonarchived cache-file digests are
historical inventory records rather than files that can be reread. A later
same-generator retry requires an additive namespace and a new quiet lease.

## Completed historical-source 1,000-message retry

After the excluded bootstrap cache was reclaimed, root granted a new exclusive
quiet/LTO lease at **2026-10-03 03:42:18 UTC**. The retry used the additive
namespace `gn03-1000-historical-retry-20261003-011228`, at source
`795193f69e4adac3d9fc980f385d5b113f0b9aa5`. It reused the exact common
generator `718aef4131813c1c6fc449e33202588fc6bf48cc330bd28cfd33a96e056f524b`,
69 historical source inputs, original driver lock, schema/tool pins, seed,
default/shared order, jobs=1, workloads, 900-second phase timeout, and original
opt-level=3/thin-LTO/codegen-units=1 settings. No generator rebuild occurred.
Both initially nonexistent profile targets remained separate.

This is a **historical generator and locked-source comparison**, not a
measurement of current production main. Main at preflight was the docs-only
commit `4b2384e728bce17601d56e43a887c354d66b9024`, whose compiled baseline was
`10bee600737b8a207159e63581bebc01b07ec2d7`. Against the 69 frozen inputs,
current main differed in `Cargo.lock`, `src/codegen/config.rs`, and
`src/text.rs`. The prelaunch preparation retains the historical inventory and
exact hash differences; none of these main files replaced the historical
capture inputs.

| Observed cost | Default | Shared |
| --- | ---: | ---: |
| Generated source bytes | 35,782,708 | 19,310,413 |
| First generation milliseconds | 117.737285 | 114.271150 |
| Unchanged generation milliseconds | 114.883383 | 120.951340 |
| Cold check seconds | 82.057415 | 75.760146 |
| Incremental check seconds | 20.070839 | 15.500649 |
| Release build seconds | 321.823111 | 312.725829 |
| Linked executable bytes | 8,896,328 | 8,896,328 |

Source volume was 46.03% lower with sharing. In this single pair, shared cold
check, incremental check, and release durations were 7.67%, 22.77%, and 2.83%
lower, respectively. **Unchanged generation was 5.28% slower**. The executables
had **equal size** and different hashes, so there is no binary-size win. The
frozen round-trip consumer does not read reflection metadata; this pair does
not establish metadata-retaining consumer cost. All raw timing and RSS results,
including the slower generation cell, remain in the reports.

All seven phases passed for both profiles, including unchanged-generation
mtime checks and the original `1\n` smoke output. Actual outer launch was
03:46:45.705825 UTC; the coordinator recorded the pair at
03:46:45.837930–04:00:35.084026 UTC. Default and shared began at
03:46:45.851292 and 03:53:50.435265 UTC. Their cache cleanup completions were
03:53:50.434549 and 04:00:35.074197 UTC. The coordinator exited zero at
04:00:35.113768 UTC. The agent observed completion at 04:00:35.212435 UTC and
immediately notified root of lease release; the notification's exact send time
was not independently captured. No quiet-window exception was reported for this retry.
The earlier small-pair overlap note and failed first 1,000 attempt remain
unchanged.

The first prelaunch process-name check refused on ten empty-argv Cargo/rustc
rows. Their `/proc` states were all `Z` (zombies), and the corrected prelaunch
snapshot found no active compiler. That refusal occurred before any measured
launch, and its disposition is retained. The snapshot covers accessible names,
arguments, states, executable paths, and working directories. Three processes denied
executable/cwd reads; environment, descriptor, and mapping reads were not part
of this snapshot. Root coordinated the exclusive lease; this limited snapshot
does not claim complete system-wide process inspection.

No resource guard failed. Maximum sampled allocated-cache bytes were
1,706,467,328 for default and 1,445,179,392 for shared, below the unchanged
2,147,483,648-byte cap. Minimum sampled global free space was 2,651,832,320
and 2,867,990,528 bytes, above the separate unchanged 2 GiB reserve. The reused
bootstrap was empty. Each completed target was reclaimed only after retaining
the linked ELF and the original coordinator's fingerprint SHA256 inventory.
These fingerprint inventories contain digests, not the original file contents.

The [retry artifacts](gn-03-sb09-1000-retry-artifacts/artifact-sha256.json)
retain the common generator, exact source/schema/tool and bootstrap records,
both corpus/generated/consumer trees and locks, linked ELFs, reports, raw child
logs, separately captured coordinator stdout/stderr, resource telemetry, and
prelaunch/launch/exit/quiet-window records. The verifier reads the archive without
executing compiler output:

```sh
python3 -B docs/evidence/gn-03-sb09-1000-retry-artifacts/check-pair.py
```

There is still one fixed-order pair per corpus, no independent reference peer,
no statistical repeat/control qualification, and no metadata-retaining consumer
measurement. The single historical retry does not qualify current-main
production compilation or establish a performance improvement. GN03 remains
open.
