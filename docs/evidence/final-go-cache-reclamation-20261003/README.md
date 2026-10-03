# Completed Go peer cache reclamation

Only the completed `/workspace/pure-protobuf/work/final-go-peer/cache` and
`/workspace/pure-protobuf/work/final-go-peer/modcache` nodes were reclaimed on
2026-10-03 at 02:34:46 UTC. The peer-proof owner confirmed both cache readers
were idle and selected F2 inputs/four ELFs were already sealed. Its later repack
uses protected top-level records/binaries and private metadata snapshots. This
cleanup is based on main a4f012ab and preserves those original protocol/source
qualification identities. No Cargo, Go, compiler, test, version, benchmark or
performance process ran as part of cleanup.

The complete build inventory covers 3,449 regular files and 257 directories;
the complete module inventory covers 10,150 regular files and 3,727 directories.
All file paths retain SHA256, size, mode, UID/GID, device/inode, link count,
blocks and mutation timestamps. Fresh full pre-delete inventories matched every
entry. After rename, full inventories still matched apart from the expected
root-directory ctime changes. The exact authorized cache inodes (device 27,
inodes 1981700 and 1981701) were renamed into unique owned quarantines, then
removed with Python's file-descriptor based symlink-resistant `rmtree`. No other
target, SDK, source tree or top-level peer namespace was removed.

The selected archive is 78,821,045 bytes with 2,281 regular payloads and SHA256
`f8735053b0315fb437997a4cc00d21fac25779cd1f2feb41891a209097f2c2e8`.
Four ordered parts reproduce those exact bytes. It preserves all 32 runtime
module source ZIPs, all 195 download-cache files (including `.mod`, `.info`,
`.ziphash`, lock/list metadata), full Go build action-index metadata and the
selected frozen build input/record/log files. Every ZIP h1 checksum was freshly
recomputed using Go's directory-hash algorithm and matched its `.ziphash`, the
frozen `go.sum` and the 32 tuples from the retained ELF module-info records.
Every expanded ZIP source file hash matched the complete module inventory.
The previous `go mod verify` PASS and original first-build failure/retry records
remain distinct historical observations, not newly executed commands.

Extracted module-tree paths were not copied separately; their inventory hashes
and the selected 32 ZIP sources are retained. Go build `*-d` output payloads
have complete inventory hashes and metadata, not full-content preservation.
This archive does not claim that every cache or SDK payload is preserved.
All existing first-failure/retry binaries, module infos, records and raw logs
remain in their protected top-level namespaces. Their bytes, task scripts,
frozen `go.mod`/`go.sum`, and four Go tool binaries were checked before and after
cleanup. The F2 owner's immutable archive holds its selected executable copies.
SDK/shared-module-cache root identities are recorded and untouched; this cleanup
does not independently inventory their entire contents.

Read-only module files retain their original modes. Only 3,594 owned read-only
directories inside the already verified module quarantine received owner-write
permission to permit deletion. Every original mode/inode is preserved; the
specific directory changes are enumerated separately. No permission change
was made to the live module cache, another cache, a file or a shared SDK.

Five process/thread scans inspected executable, cwd, argv, environment,
file descriptors and maps. They found zero references on readable surfaces.
Before/after preservation they recorded 605 permission-limited and 486 transient
surfaces; the three later scans recorded 607 and 488. Nontransient unreadable
live processes were known root-owned Docker helpers, with other nontransient
failures belonging to zombies. Raw argv/environment contents were not saved.
Container namespace visibility, permissions, transient processes, PID reuse,
future writers and non-atomic enumeration remain limits. The work-only owner
advisory lock coordinates this script and is not a Go cache lock. These checks
are bounded observations, not a global atomic absence guarantee.

Unique nominal allocated `st_blocks` for both caches, including directories,
were 1,035,022,336 bytes; regular-file apparent lengths totaled 983,155,123 bytes.
Device/inode accounting is distinct from shared overlay/CoW physical extents.
Global available space increased by 1,026,002,944 bytes between the immediate
pre-delete and post-delete observations. Concurrent agents could write elsewhere,
so that difference is not an attributable physical reclamation measurement.
Archive writes required a 2 GiB global free-space floor. No uncompressed copy
of either complete cache was staged.

Run `python3 verify.py .` for read-only verification of all bound artifacts,
complete inventory/accounting equality, archive payloads, ZIP/expanded-source
hashes and frozen module tuples, exact-node deletion scope, directory permission
changes and bounded process records. The original caches need not exist.
Do not rerun `reclaim.py`; any future build requires a newly authorized cache.
