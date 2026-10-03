# Completed embedded-backend cache reclamation

Only `/workspace/scratch/work/h2-embedded/target` was removed, on
2026-10-03 at 01:36:33 UTC. This completed cache was produced by the topology
qualification at source 604f4c2e; the snapshot checkout was ec364078. Cleanup
changes neither that source nor its source-qualified test, package or consumer
results. The subsequent supported-Clippy, dependency-range and test-fixture
source overlays are separate commits with separate qualification ownership.
No compilation ran as part of cleanup.

The archive retains hashes, lengths and inode/allocation records for all 1,615
regular cache files. It preserves the full contents of all 1,240 files classified
as fingerprint, dependency, generator or other metadata, and four current
consumer ELFs. The other 371 compiler artifacts have hash records rather than
full-content copies in this archive. The qualified default 1.85 and Tonic 1.88
unit ELFs remain in the immutable topology archive and separate work copies;
this cleanup checker verifies their hashes in that topology archive too. Earlier
cache removals have their earlier bounded hash/phase records; this capture does
not retrospectively claim their metadata contents were preserved.

The shared toolchain/Cargo installation, owned Miri sysroot, original topology
archive and qualified source/raw evidence remain intact. Three instantaneous
procfs inspections found no active owner of this leaf's target, and the leaf
was idle. Two procfs entries were permission-limited. Inspection excludes the
capture process and its launch ancestors and cannot guarantee against races or
future writers; it is not a filesystem-wide lock.

An authorized locked Cargo metadata audit, performed between snapshot and
reclamation, refreshed only `.rustc_info.json` through compiler introspection.
It did not compile project sources. The first pre-deletion verification stopped
on this changed hash and removed nothing. Both complete old/new versions and
the failed verification are preserved. The updated manifest verifies every
other cache hash unchanged before deletion.

The sum of regular-file apparent lengths was 1,369,837,783 bytes after that
refresh. Deduplicating by device/inode gives 1,305,510,959 apparent bytes and
1,310,519,296 allocated bytes, plus 1,642,496 directory allocated bytes. The
allocation measurement uses `stat.st_blocks`; it does not measure shared extents,
copy-on-write ownership or filesystem quotas. Summing apparent paths includes
hardlink aliases. Filesystem free space was 3,557,826,560 bytes at capture and
4,563,963,904 bytes immediately after removal; concurrent agents' disk activity
means this difference is not a direct measurement of reclaimed bytes.

`python3 check-artifacts.py` checks the archive/member manifest, every retained
metadata and consumer-ELF byte hash, all cache accounting, the single metadata
refresh, process records and reclamation scope. It checks qualified unit ELFs
against the adjacent original topology archive. It does not recreate the target
or invoke Cargo, rustc, tests or performance capture. The raw archive is
20,078,931 bytes with 1,258 regular members and SHA256
`bac4f74ac4002f9882aae32151e0df15c290a48d7cbc73fff7fb919a9ed9c810`.
