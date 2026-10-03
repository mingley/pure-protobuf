The historical GN03 source-matched `cg19-generator` debug drivers now build
successfully with the accepted standalone graphs. This is excluded benchmark
setup evidence. GN03 remains in progress; release builds, screens, repeated
factor measurements and performance qualification are **NOT_RUN**.

The source pair is baseline `4b2384e728bce17601d56e43a887c354d66b9024`
and candidate `5fe226b015a2c2bc667db976390b8337db6e9a2e`. Each retained
projection contains 1,027 exact regular Git blobs, including the unchanged
codegen harness and driver API. The independent auditor compared all 2,054
projected files with those commits and retained their Git object IDs, hashes
and mode maps. These historical inputs are distinct from current main.

| Capture | Actual result | Preparation |
| --- | --- | --- |
| `driver-bootstrap` | Cargo exit 101: offline `bytes` package unavailable with omitted Cargo/Rustup homes | Original `26fe775f` |
| `driver-bootstrap-002` | Cargo exit 101: seeded standalone lock requires an update | Original `26fe775f`; homes corrected |
| `driver-graph-amendment-001` | Four metadata commands pass: normalization and subsequent locked acceptance for each source | Accepted `125a1a43` |
| `driver-bootstrap-003` | Both actual debug builds pass, then their complete caches are preserved and retired | Accepted `125a1a43` |

The graph amendment operates only in separate work directories. Both accepted
locks retain all 14 original registry name/version/source/checksum tuples;
their complete resolved nodes, edges, dependency kinds and feature sets match
after the two declared source/driver path substitutions. The original seeds,
preparation, failed commands, helper versions and raw outputs remain available
in the capsule. The driver manifests and Rust bytes are unchanged.

The passed commands use actual stable Rust/Cargo 1.99, pinned protoc 35.1,
explicit Cargo/Rustup homes, jobs 1 and `--offline --locked --bin
cg19-generator`. They use the existing default debug profile, without recorded
compiler, wrapper or profile overrides. The raw build bookkeeping, process
sessions, source/helper/tool checks and fixed 2 GiB allocated-cache cap and
available-space floor are retained. This evidence makes no comparative wall
time, instruction, allocation or numeric performance claim.

Cargo compiler-artifact JSON was **not** captured: the helper selects
`target/bootstrap/debug/cg19-generator` after successful native `--bin` build,
checks its ELF magic, copies and hashes it, and matches the retained copy to
the complete cache manifest. The two local binaries are 64,334,520 and
64,322,408 bytes; their full hashes are indexed in [index.json](index.json).
They are not committed as large binary blobs.

Both full cache archives were independently streamed and checked against
their complete manifests: 603 members and 549 regular paths each, including
all 388 ELF paths per cache (object files and shared libraries included),
fingerprints, build outputs, dependencies, rlibs, modes, ownership and hardlink
topology. There are no hash-only cache payloads. Held OFD plus flock anchors,
independent blocked contenders after archiving and before retirement,
unchanged source checks, fresh inode-qualified live checks and all removed
paths are retained. Process visibility is limited to accessible `/proc`;
permission denials remain explicit. Both retired namespace roots are absent.
The large cache archives stay local and are indexed by exact hashes.

The prior tool-probe envelope contains the sentence “No Cargo build/driver
bootstrap/native bench/release/LTO/capture was executed.” It is preserved
verbatim as a statement about that earlier preparation stage; the new `003`
BUILD records establish the later setup passes. The probe's selected tool,
config and immediate sysroot records do not establish a complete compiler,
linker, header, loaded-library or system closure for these builds. The reused
ordinary source report is a prerequisite, not an additional test execution.

Run the portable source/graph/build-record checker from any checkout:

```sh
python3 docs/evidence/gn03-driver-bootstrap-20261003/check.py
```

To repeat the full local cache and immutable driver readback, add the original
proposal directory:

```sh
python3 docs/evidence/gn03-driver-bootstrap-20261003/check.py \
  --payload-root /workspace/scratch/work/prost-contiguous/work/gn03-factor-performance-proposal-20261003
```

Neither command launches Cargo, a compiler, protoc, a driver or a performance
tool. Default checking verifies the indexed capsule; it explicitly labels the
large local payloads as not read. The independently executed full readback is
recorded in [verification.json](verification.json).
