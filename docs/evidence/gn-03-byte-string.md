# GN03 shared descriptor byte-string ordinary qualification

Frozen baseline `4b2384e728bce17601d56e43a887c354d66b9024` and candidate
`5fe226b015a2c2bc667db976390b8337db6e9a2e` passed the bounded ordinary gates below.
The shipping change uses one escaped Rust byte-string token for the active opt-in
shared metadata owner. Standalone/default arrays and public `FILE_DESCRIPTOR_SET:
&[u8]` remain unchanged. No default, wire, dependency or public API change is claimed.
The tested snapshots are deliberate historical source points; this record does
not label them current production compiler inputs.

The source audit corrected the prototype to select the new renderer using actual
`share_descriptors` activation, preserving inactive single-output bytes. Raw
array-owner and byte-string-owner prefixes are counted separately, excluding
aliases. These were source review corrections before execution, not runtime reds.

## Actual ordinary gates

Six sequential baseline/candidate stages ran native Rust1.99 stable, Rust1.85
MSRV and Rust1.88. Cargo, rustc and Clippy-driver probes retain fresh argv, separate
stdout/stderr and exit records. Cargo-Clippy is hash pinned and qualified through
proper `cargo clippy` commands; it is not invoked as a standalone version probe.
No compiler fallback, lock update or `--ignore-rust-version` was used.

- Six all256-byte/empty/wrap hex renderer unit executions passed. The candidate
  compiled all256-byte/empty byte-string oracle passed on all three toolchains.
- The same strengthened existing seven-test suite passed on both source versions
  for each toolchain:42 test passes,42 actual successful generated consumer
  executions and6 retained expected missing-helper compile failures(exit101).
  Native1.85 qualifies MSRV; native1.88 supplies actual strict qualification.
- Stable and native1.88 strict shipping library/CLI/focused-suite Clippy passed
  four runs with `-D warnings`; seven generated consumer profiles passed per run
  (28 actual strict consumer checks).
- The unchanged harness ran66 tests:65 passed and1 optional case skipped. Its
  intentional UNQUALIFIED fixture message remains in the raw log.

Both sides execute the same independent compiled decimal-array FDS oracle.
Assertions cover metadata aliases before lazy pool initialization, fixed binary
bytes `[8,41]`, scalar JSON/text laziness, expanded Any initialization, schema
files/field types, stable pool/descriptor Arc identity and alias slice pointers.
Consumer lockfiles, oracle sources and manifests after only absolute runtime-root
normalization match each same-tool pair. The seven successful profiles retain
shared registry, explicit flat helper, no-reflect lean runtime, single flat
include and default root message/enum/nested-module behavior. Shared-pool/config
conflict and inactive/no-option/default identity assertions remain in the suite.

## Exact30-output default and shared-source proof

New baseline and candidate CLI plugins were compiled and copied from fresh
hash-guarded snapshots. Both replayed the original immutable seeded request/FDS
corpora. The archived original baseline plugin was a third golden comparator;
the old795/718aef performance driver was not substituted for the new matched
baseline. All30 default filenames and bytes equal each other and the archived
golden default output. The original308 count is archive membership, not outputs:
the actual default output count is3+6+21=30.

| Corpus | Default files | Default bytes (both versions) | Baseline shared bytes | Candidate shared bytes |
| --- | ---: | ---: | ---: | ---: |
| small | 3 | 135,821 | 129,486 | 127,877 |
| 100 | 6 | 2,313,461 | 1,947,481 | 1,924,911 |
| 1000 | 21 | 35,782,708 | 19,310,413 | 19,096,278 |

Each shared corpus has one raw owner(array baseline, byte-string candidate),
excluding aliases. All other application/registry source bytes and helper pool
body bytes match. Independently canonicalized descriptor bytes match both owners.
These are source-volume/equality observations, not performance or release size
acceptance. Debug copied plugins have source/tool/hash provenance from actual
`cargo build --bin`; compiler-artifact JSON and a complete native compiler closure
are absent. They are ordinary test tools, not qualified release BUILD artifacts.

## Resource and retention proof

Root's literal ordinary compiler lease was
`2026-10-03T05:42:54.379613+00:00`. The campaign ran
`05:43:45.259227`–`05:48:10.502938`UTC, exited0 and released the compiler lease
at the observed second-precision clock `05:48:25 UTC`. This was an ordinary
compiler lease with other agents source/proof-only, not a quiet performance lease.

Fixed jobs1/offline/locked gates used an aggregate allocated cache cap2GiB and
global free reserve2GiB. The369 raw samples observed maximum owned allocation
111,292,416 bytes and minimum global free5,099,614,208 bytes, with zero guard
failure samples. These bounds are sampled; between-sample peaks and exclusive
overlay backing-store allocation are not certified. After six verified exact
target retirements, only six empty target roots(24,576 allocated bytes) remained.

Every completed cache file was hashed; original fingerprint contents, all actual
ELFs, generated fixture/source/manifest/lock files and compiler logs were archived
and member-verified before retirement. Other non-ELF compiler cache payloads are
hash inventories only: `.rlib`, `.rmeta`, `.d` and build output outside the retained
`gn03-tests` fixtures and fingerprint directories are not full dependency/build
payloads. The exact inventory-minus-retained paths/counts and direct ELF-magic
classification are published in
[hash-only-cache-audit.json](gn-03-byte-string-artifacts/hash-only-cache-audit.json)
and recomputed by the verifier. Visible /proc cache ownership and source hashes were
checked again before deletion. Inaccessible process internals/maps/environments
are not proved; normal disappearing process paths and zombies are distinguished
from live compilers. All prior GN archives remain immutable.

Retirement used the reviewed namespace/path/source/hash/visible-process rechecks,
then `shutil.rmtree`. It did not hold advisory Cargo locks or use fd/inode-qualified
retirement; no such stronger guarantee is claimed.

## Verify and scope

The archive contains the runner/config/lease/frozen plan, source snapshots,
actual argv/environment/tool/phase records, all default/shared requests and
responses, copied plugins, six nested completed-cache evidence archives and the
work-only orchestration checks. Published hashes are listed in
[artifact-sha256.json](gn-03-byte-string-artifacts/artifact-sha256.json).
[qualification.json](gn-03-byte-string-artifacts/qualification.json) retains the
actual gate/retirement results; [provenance.json](gn-03-byte-string-artifacts/provenance.json)
states source/tool/retention limits. Paths inside raw records identify the original
workspace; compiler replay requires the pinned source/tools and matching paths.
The standalone verifier does not extract or launch compilers:

```sh
python3 -B docs/evidence/gn-03-byte-string-artifacts/check-evidence.py
```

Release/LTO, matched small/100/1000 performance factors, linked consumer release
size, repeated large-corpus and independent-host qualification remain not run.
Parent GN03 binary/check/repeated-large acceptance remains open. A future separate
metadata-retaining consumer must not replace the original controls to manufacture
a size win. No fastest-in-world claim follows from this ordinary proof.
