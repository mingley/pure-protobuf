# GN03 opt-in source descriptor sharing qualification

The implementation at `c7d2552e9c1b4229126aa172cd122343a292c7b6` adds
`Config::shared_descriptor_set(true)` and the matching plugin option. The option
defaults to false. Reflective requests with multiple canonical output targets
can install one raw FileDescriptorSet and one lazy
`OnceLock<Arc<DescriptorPool>>` in `__pbrs_shared_descriptors.rs`. Each generated
file keeps its public `FILE_DESCRIPTOR_SET` constant as an alias and delegates
pool access to that owner. The existing root `include_file` registry installs
the helper module. Flat consumers must install that module explicitly at their
crate root.

Default generation remains byte identical. Single-output requests with the flag
also remain byte identical and do not emit a helper. `no_reflect` emits no
metadata. Active sharing rejects `shared_pool`, helper filename/module/API-name
collisions, registry/application filename collisions, and a registry path that
is not a root filename. These checks do not change inactive/default generation.
No runtime behavior, wire schema, dependency, feature default, or bundled binding
was changed. The three production source areas are config, descriptor generation,
and reflection metadata emission.

## Source-volume proof

Base: `796005d373ddef825a9e185f433c7f879a5ce51b`. The frozen baseline and final
plugins generated the actual seeded SB09 corpora from
`bench/codegen/run.py` (`CORPORA`, `DEFAULT_SEED`, `render_proto`), using pinned
protoc 35.1, `--include_imports`, and the normal default without source info.
The request contains every target and the complete descriptor graph. Baseline
and final default use `stubs=none`; opt-in adds `shared_descriptor_set=true`.
Counts include every response file, including the registry and new helper.

| Corpus | Requested files | Default files → shared files | Default bytes → shared bytes | Default lines → shared lines | Source-byte reduction |
| --- | ---: | ---: | ---: | ---: | ---: |
| small (6 messages) | 2 | 3 → 4 | 135,821 → 129,486 | 2,330 → 2,257 | 4.66% |
| 100 messages | 5 | 6 → 7 | 2,313,461 → 1,947,481 | 39,915 → 35,604 | 15.82% |
| 1,000 messages | 20 | 21 → 22 | 35,782,708 → 19,310,413 | 549,474 → 355,645 | 46.03% |

For all three corpora, every final default filename and byte equals the baseline;
the shared output generated twice is byte identical and has exactly one raw
descriptor literal. These are source-volume observations only. No cold-check,
release binary-size, timing, or memory improvement is claimed. A linker may fold
or remove metadata. GN03 performance acceptance remains open until the actual
SB09 check/build/binary-size cells are qualified under a separate quiet lease.

## Correctness and strict-consumer checks

Exact commands and log paths are in
[commands.json](gn-03-source-artifacts/commands.json). Raw logs, exit statuses,
fixtures, request/response binaries, descriptor graphs, generated outputs,
consumer sources and locks, and baseline/final source capsules are retained in
the artifact archive.

At frozen source:

- Focused shared-descriptor suite: **7 passed**. It covers typed/plugin option
  equality, default/single-file identity, repeated/reordered generation,
  reflection graph and shared Arc identity, renamed runtime, real expanded Any
  JSON/text round trips, and lazy pool construction. Scalar binary/JSON/text work
  leaves the pool empty; expanded Any initializes it. It also covers no-reflect
  lean-runtime compilation, flat inclusion with/without the explicit helper,
  and configuration/path/root-type/nested-module/server-trait collisions.
- Strict Clippy: shipping codegen library, CLI, focused suite, and **7 generated
  consumer profiles** passed with `-D warnings`. Profiles cover shared registry,
  flat explicit helper, no-reflect, single flat include, and the default root
  message/enum/nested-module collision fixtures.
- Plugin suite: **46 passed, 2 ignored** optional protoc 36.1 cases.
- Relevant noncold onboarding: **13 passed, 1 ignored, 2 filtered**. The ignored
  case is the optional pinned 35.1 descriptor-consistency case. The two unchanged
  full-cold packaged-core/adapter cases were explicitly deferred to integrated
  acceptance rather than claimed here.
- Strict codegen rustdoc, formatting, and whitespace checks passed.
- Safe pinned regeneration `--check`: **13 bindings passed**, with no generated
  source change.

Before the final active-only collision hardening, the broader documentation,
dynamic, and builder suites passed **25 + 36 + 32** cases. The final default byte
identity proof supports their unchanged default behavior, but this record does
not label that earlier run as a rerun at the frozen commit. Integrated
conformance, the two deferred cold onboarding cases, and performance acceptance
belong to the main integration qualification.

## Retained failed attempts and review corrections

The old source rejects the new option (`UnknownParameter`, exit 101), establishing
the missing-sharing baseline. A peer review then found that a root message,
enum, or nested module could shadow the reserved helper. The default message
consumer compiled, while the pre-fix opt-in unexpectedly accepted the collision;
that red replay and source snapshot are retained. A separate pre-fix replay
shows the registry/application filename collision was accepted, potentially
dropping one output during deduplication. Both now return explicit generation
errors, including root emitted service trait conflicts.

Fixture-development failures are also retained: an initial test used private
wire helpers, the first laziness assertion incorrectly expected scalar JSON/text
to initialize the pool, and one root enum fixture used an unavailable constant.
The final fixture uses local wire construction, expanded Any to exercise pool
initialization, and the actual generated enum API. Preliminary source-volume
output had an unindented helper body; the final body is 16 bytes larger and the
table above uses only the frozen final output. These preliminary/failing logs
are not passing qualification evidence.

## Provenance, replay, and resource limits

[provenance.json](gn-03-source-artifacts/provenance.json) records the baseline and
final source hashes, manifests/lockfile, unchanged corpus harness, plugin hashes,
actual Cargo/rustc/protoc binary hashes and versions, and pinned protobuf source
and build stamp. The final gates use a work-only Cargo wrapper that forces
`CARGO_BUILD_JOBS=1` even where existing nested consumer helpers override it. The
wrapper is archived and hashed. This was ordinary correctness validation, not
a performance measurement.

Completed consumer caches were archived before reclamation. The final noncold
onboarding cache briefly reached about 2.2 GiB including the reused base target,
exceeding the requested 2 GiB owned-cache budget; it was reported and reclaimed
immediately after the running command finished. About 691 MiB of reused base
target remained. This record does not claim the cache cap was continuously met.

The 308-member compressed raw archive includes all source-volume inputs/outputs
and failure/pass logs. It also contains completed consumer source/lock/fingerprint
archives and executable-hash inventories, rather than retaining their build
caches. Paths inside logs describe the original isolated worktree and are not
required to verify the published artifacts. The archive is read without
extracting files by the standalone verifier:

```sh
python3 -B docs/evidence/gn-03-source-artifacts/check-source-volume.py
```

The verifier checks every archive member's hash/size, generated-file metrics,
source capsules, baseline executable hash, same descriptor inputs, default byte
identity, shared repeated byte identity, source totals, and single metadata
ownership. [artifact-sha256.json](gn-03-source-artifacts/artifact-sha256.json)
lists the published artifact hashes; the archive member manifest supplies hashes
for its contents. Re-running actual generation additionally requires the pinned
tools and the corresponding source commits, using the retained
[record-source-volume.py](gn-03-source-artifacts/record-source-volume.py).
