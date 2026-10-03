# GN03 SB09 shared-descriptor profile and measurement preparation

This record qualifies the benchmark wiring at
`dea20f7ae6ec5d33fe4d7cb5fe64004cf6263247`. The small corpus now has one
**unqualified local diagnostic pair**; numeric cold-check and linked-binary
results for **100 and 1,000 messages are not_run**. Qualified performance
acceptance remains open. The
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
The reused excluded bootstrap is about 691 MiB. The cache limit is 2 GiB with
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

The first 100- and 1,000-message pairs and repeat/control qualification remain
pending separate root capture leases. GN03 remains open.
