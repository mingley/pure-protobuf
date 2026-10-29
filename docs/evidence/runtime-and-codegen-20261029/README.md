# Runtime and codegen measurements, October 29

This round changes the representation of embedded descriptor bytes in generated
Rust and selects repeated-tag and varint fast paths. The literal change reduces
generated source size on the three measured schemas. The
measurements do not establish that every gRPC or protobuf benchmark beats
tonic/prost, full feature parity, or a world ranking. This round measured the
protobuf kernel and code generation; it did not run a new full RPC campaign.
Codec gains alone do not establish an end-to-end RPC speedup.

[Timing rows](timing.csv) · [Resource rows](resources.csv) ·
[Source and validation](results.json) · [Raw evidence](raw.tar.gz) ·
[Benchmark guide](../../benchmarks.md)

The integrated measurements use source `0a770792d8db65c8bf69677e2f91431c92640e63`;
the final validation source is `bc8cec386a3628da272ae97ab4bc8bcee18fd7c9`. Their frozen inputs differ only in two test files:
`tests/plugin.rs` and `tests/proof/repeated_tag/build.rs`. The
[October 28 record](../optimizations-20261028/README.md) retains the earlier
measurements and rejected candidates.

## Descriptor output

The generator now writes the descriptor set as one Rust byte-string literal,
instead of a separate array expression per byte. The public
`FILE_DESCRIPTOR_SET` type and bytes are unchanged. Default output remains
self-contained for a file that has no other generated-file dependencies.
This intentionally changes generated source text across generator versions.
Previously compatible generated code does not need regeneration solely for
the new spelling.

Regenerate application bindings to use the repeated-tag loop optimization.
The varint runtime fast path also benefits compatible existing bindings without
regeneration. Bundled bindings are regenerated for the repeated-tag change.

| Schema | pbrs before | pbrs after | Prost | pbrs reduction |
| --- | ---: | ---: | ---: | ---: |
| 6 messages, 2 files | 140,322 B | 137,104 B | 2,603 B | 2.3% |
| 100 messages, 5 files | 2,371,393 B | 2,258,543 B | 41,797 B | 4.8% |
| 1,000 messages, 20 files | 36,310,287 B | 32,027,587 B | 412,622 B | 11.8% |

These are total generated Rust bytes for the descriptor-only candidate. The
Prost output does not include the same generated reflection, JSON, and text facilities, so its size is a useful
observation, not a feature-equivalent comparison. For each pbrs output, the
comparison decoded both literal forms and verified identical descriptor bytes;
all source outside those literals was identical. Tests also compare all 256
byte values with a compiled Rust literal and compile a standalone consumer.

The final combined generator also changes repeated-field parsing. Its output
is 136,464 B, 2,248,949 B, and 31,919,839 B for the respective schemas.
All 27 embedded descriptor sets match the descriptor-only candidate byte for
byte. Each generated proto source changes because these schemas include
repeated fields. The descriptor-only consumer timings below must not be read as
a measurement of the final combined generator's output.

The bundled SDK uses `shared_pool=true` and has no embedded descriptor literal.
A pinned, isolated regeneration of the descriptor-only candidate reproduced
all 13 prior bindings exactly; this representation change alone does not
require updating them. The combined repeated-tag generator change updates
eight of the 13 bundled bindings; the other five remain byte-identical.

## Generation and consumer builds

The following generation and consumer measurements use the descriptor-only
candidate; they do not measure the final combined source. Release generation
measured five paired runs using already-built generator
binaries and the same input schemas. Fresh-generation candidate/before median
elapsed-time ratios were 1.011, 0.967, and 0.992 for 6, 100, and 1,000 messages;
the observed pair ranges all cross 1. The change does not establish a generator
speed improvement in these runs. Candidate/Prost median paired ratios were
1.126, 0.941, and 0.800 for the same schemas, subject to the feature difference
above and observed pair variability.

The final combined generator was also measured against the original baseline.
For fresh generation, combined/baseline paired medians were 0.992, 0.834, and
1.130 on the three schemas, with ranges 0.973–1.170, 0.728–1.087, and
0.935–1.615. All ranges cross 1. Its paired medians against Prost were 1.150,
0.865, and 0.854, with the output feature differences described above.

The consumer comparison uses the already-generated source and prebuilt, locked
dependencies. Before each timed invocation it removes the consumer artifacts,
then verifies Cargo rebuilt only the consumer. It excludes generation,
dependency builds, and source copying. Thus it is a dependency-warmed consumer
rebuild, not a fully cold build. Three paired runs on one CPU gave:

| Schema and operation | Before median | After median | Prost median | After/before paired median (range) |
| --- | ---: | ---: | ---: | ---: |
| 6 messages, check | 0.2875 s | 0.2717 s | 0.0790 s | 0.931 (0.926–0.983) |
| 6 messages, release build | 8.6225 s | 8.6235 s | 3.1456 s | 0.996 (0.991–1.026) |
| 100 messages, check | 4.0778 s | 3.9722 s | 0.5774 s | 0.982 (0.957–0.991) |
| 100 messages, release build | 24.5290 s | 24.2323 s | 7.8621 s | 0.988 (0.967–1.029) |

The check results improved in all three pairs for these inputs; release-build
results overlap. The 1,000-message consumer was not measured in this screen.
Release binaries ran successfully. The shared host and small number of pairs
limit generalization.

A separate three-pair check compares the original baseline output with the
final combined output, using the same prebuilt pbrs runtime for both. It does
not measure the final runtime's own compilation or a release build:

| Schema | Baseline median | Combined median | Prost median | Combined/baseline paired median (range) |
| --- | ---: | ---: | ---: | ---: |
| 6 messages | 0.2887 s | 0.2716 s | 0.0758 s | 0.941 (0.925–0.989) |
| 100 messages | 4.2097 s | 4.2869 s | 0.6226 s | 1.035 (0.944–1.037) |

All three small-input pairs favor the combined output; the 100-message range
crosses 1. Prost's output has fewer facilities, as noted above.

## Combined runtime screen

The integrated source was compared with the prior pbrs implementation and a
fresh Prost control. Among 52 measured CPU rows, paired medians favored pbrs
in 49: 32 rows cover parse and full read across eight fixtures and two storage
modes, four cover partial getters, and 16 cover encode operations. The three
remaining losses against Prost were lazy full reads at nested depths 8 and 64,
and the lazy first-child read at depth 8. These counts describe measured
medians on this host, not wins for every input or every pair.

For example, owned `rpc_mixed` full read had a 0.854 paired median ratio to
prior pbrs (range 0.844–0.879) and 0.866 to Prost. Owned sparse parse regressed
to 1.104 times prior (1.051–1.139). With the separate non-LTO build, packed
parse regressed to 1.136 times prior (1.032–1.278). All 16 standard owned
parse/full-read paired medians still favored pbrs over Prost. In the separate
repeated-field screen, owned parsing of 1,024 numbers was 0.826 times prior
(0.795–0.835), and owned mixed-64 parsing was 0.892 (0.887–0.946). Lazy
parsing of 1,024 children remained slower than Prost at 1.168 times
(1.114–1.276). One lazy small-roundtrip row changed from a prior win to a
loss. The full rows and controls are required to interpret the tradeoffs.

## Validation

At `bc8cec38`, the all-feature workspace run passed 3,994 tests with zero
failures and nine existing ignores. Strict lints, five Rust 1.85 profiles,
mixed-peer and message-presence tests, codegen, pinned descriptors, and
regeneration checks passed. The default pinned protobuf 35.1 suite ran required
conformance twice and recommended once: each passed 5,631 binary/JSON and 909
text cases, with no failures or skips, through Edition 2023. The default run
recorded a clean source. A fresh owned-profile run passed the same cases using
a deliberate 13-binding generated overlay; its source was therefore dirty by
design. These are finite test scopes, not full Prost equivalence.

Some earlier passing focused checks at `d6124b79` were reused after confirming
all other tracked non-Markdown inputs matched `bc8cec38`; the repeated-tag
proof and fixture Clippy were rerun at `bc8cec38`. The archive retains the
initial plugin failure from an obsolete source assertion, a follow-up
consumer type-check failure, a zero-test proof invocation, and an earlier
fixture Clippy failure. Later corrected tests pass; the failed attempts are
not counted as successful validation.

## Rejected experiments

| Candidate | Evidence and decision |
| --- | --- |
| Packed map index | Rejected: warm ordinary full-iteration loops over distinct keys at 16, 64, and 256 entries took 5.471, 7.197, and 7.708 times prior CPU. Separate cold entry-list construction/read/drop measured 1.425, 1.182, and 1.149 times prior. These map controls do not measure Prost decoding. |
| Compact map index | Rejected: the same warm ordinary loops took 2.487, 3.186, and 3.385 times prior CPU. Separate cold entry-list construction/read/drop measured 1.116, 1.049, and 1.029 times prior. Assembly showed the duplicate helper taking `&mut MapIter`, allowing iterator state to escape; the compiler reloaded and checked state in the outer loop. Passing immutable items, index, and cursor separately is an untested idea. |
| Bounded repeated-run reservation | Not shipped: a targeted allocation proof reduced requested bytes for one short string followed by an unrelated 64 KiB suffix from 939,648 to 65,928, but 1,024 strings increased allocation calls from 3 to 8 and requested bytes from 41,040 to 80,720. These are allocator-request counters, not RSS or CPU measurements. |
| Receive buffer strategy | Rejected after the isolated microprobe: CPU rose by 1.30–1.56 times prior for 1 MiB inputs and 1.74–1.86 times for 8 MiB inputs, despite fewer requested allocation bytes. This is a microprobe, not a full RPC endpoint measurement. |

The preceding round also rejected generated-output move-and-shrink, several
map and borrowed-wire variants, and a lazy validation enum; its evidence page
contains their exact results. A plain generated-output move was also rejected
after requested-live peak and retained-capacity regressions. The allocator
counter measured requested bytes and cannot establish RSS or realloc's internal
copy behavior.

## Evidence

A varint microprobe showed faster one-byte decoding but slower truncated
inputs; it is insufficient to infer an end-to-end codec result.

The first isolated SDK regeneration reused a shared Cargo target's top-level
plugin executable and picked up unrelated generated-code changes. Its outputs
are invalid and retained separately. The accepted check rebuilt the plugin in a
dedicated target, recorded Cargo's `fresh: false` artifact, and compared all 13
outputs. A consumer timing capture was deliberately interrupted and excluded;
the runtime dependency's frozen source hashes were checked before continuing.

The linked results and archive retain raw inputs, source and binary hashes,
per-pair data, commands, test logs, and invalid attempts.

## Reproduce the record

Run the archive and index integrity check from the repository root:

```sh
python3 docs/evidence/runtime-and-codegen-20261029/check.py
```

The [manifest](manifest.json) inventories every retained archive member and
checksums the [results](results.json), [timings](timing.csv), [resources](resources.csv),
and [raw archive](raw.tar.gz). The archive's `source/manifest.json`,
`source/baseline-overlays/`, and `source/overlays/` reconstruct the nine frozen
variants. Their baseline is `9287d474d3062911af3e9a65612a9d986d9be530`; its
retained shipping inputs match the prior measured `ce0a06e5` source in the
recorded scope. This input equivalence does not establish equal timing from
separately built binaries. With that baseline available in the repository, a
fresh reconstruction of the measured integrated source is:

```sh
round29_tmp=$(mktemp -d)
tar -xzf docs/evidence/runtime-and-codegen-20261029/raw.tar.gz -C "$round29_tmp" source
python3 "$round29_tmp/source/reconstruct.py" "$PWD" "$round29_tmp/integrated" round29-integrated
```

The archive also retains `work/round29-owned-conformance/capsule/` with its
own reconstruction instructions and before/after hash checks. Commands, input
and binary hashes, logs, failed attempts, and per-pair measurements remain in
the archive and results. These records apply to the named sources and host.

## Remaining work

The large generated-source consumer build remains unmeasured. Lazy nested
reads and child parsing, sparse parsing, non-LTO packed parsing, map construction and warm access, and transport workloads still have
measured losses or unresolved variation. The latest full RPC comparison remains
the October 28 record at `ce0a06e5`: longer 8 MiB identity confirmation cost
1.494 times tonic client CPU and 1.654 times server CPU, with wide pair ranges.
Resolve those with representative holdouts and separately verify missing
feature-parity and deployment cases; the current data do not establish a
universal performance lead.
