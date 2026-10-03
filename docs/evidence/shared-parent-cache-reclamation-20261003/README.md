The completed historical SB32/TC32b debug target at
`/workspace/scratch/work/sb32/target` was reclaimed on 2026-10-03 at
02:03:16 UTC under the coordinator's explicit authorization. Only that exact
owned target inode (device27/inode2002079) was removed. No compiler, release
build, benchmark capture, shared SDK/Cargo/Miri/sysroot cleanup or shipping
source change occurred. This additive proof is based on
`f37b68ce0f426bccf025858ac3339481eb46376d` and qualifies cache preservation and
cleanup only; it is not current-release or performance evidence.

The complete before and fresh pre-delete inventories match for all 2,564
file paths and 573 directory paths, including hashes, sizes, modes, ownership,
link counts, device/inode identity, blocks and mutation timestamps. The
authorized directory was renamed to a unique owned quarantine so a later
new target at the original pathname could not be deleted accidentally.
The post-rename inventory also matches, with only the expected root-directory
ctime change accepted. The inode-checked quarantine was then removed using
Python's file-descriptor based symlink-resistant `rmtree`.

The 97,008,678-byte lossless archive has SHA256
`51d9e9478a4a9450d85e61668b756cdd636e38fe29bc5bff5059033c099d1011`.
Its four ordered parts concatenate to the exact original gzip bytes; every
part has an independent hash/size. All 2,219 members were verified twice
against the input inventory before deletion, including hardlink payload
resolution. It preserves verified copies of all six devloop ELF paths/five
unique source inodes, all 1,270 full fingerprint files (not only JSON), all
633 build files including generated `out` data/build scripts/logs, and all
298 dep-info files. The original outside-cache SB32 executable/generated
service copies were freshly hash-checked and remain intact. The byte-pinned
tools also remain intact. Their historical version outputs are preserved;
no new version process ran here.

The archive additionally preserves the existing immutable TC32b/SB32 raw and
tool/source proof files and full compiled-source-root Git archives for
`8ca03da51ab3723f53858800ca5b8e2a33d0f448` and
`5a263139f0963488bab9e34b5282803abc3d9138`. Original historical failures and
debug-only qualifications remain unchanged. Every devloop executable ELF is
preserved; eleven other `.so` compiler artifacts remain hash-inventoried.
The two test ELF hashes without independently established source association
are not assigned a guessed source. `raw-payload-aliases.json` identifies the
two exact captured Git-archive stdout payloads already retained inside the
archive, avoiding redundant uncompressed copies in this sealed proof.

Four fresh process/thread scans covered executable, cwd, argv, environment,
file descriptors and maps before/after preservation and before/after rename.
They found zero references on readable surfaces, excluding only this
cleanup process's explicitly recorded own advisory guard-lock descriptor.
All readable-surface digests and exact failures are retained. Each scan
recorded 589 permission failures and 470 transient/disappearance failures;
the nontransient unreadable live processes were known root-owned Docker
helpers, while other nontransient failures belonged to zombie processes.
The first scan covered 271 process/thread views and the later scans 270.
The process namespace, Docker permission restrictions, process creation,
PID reuse and advisory locking limit observation. These checks do not
establish a global atomic guarantee. Raw credentials and environment values
were not written into the proof.

Allocated/apparent and global free-space values are deliberately distinct:

| Quantity | Bytes |
|---|---:|
| Unique regular-file apparent size | 1,950,040,186 |
| File-path apparent size including hardlink aliases | 2,189,839,546 |
| Unique regular-file allocated `st_blocks` | 1,954,201,600 |
| Unique directory allocated `st_blocks` | 2,412,544 |
| Total unique inode allocated `st_blocks` | 1,956,614,144 |
| Observed global available immediately before deletion | 2,813,493,248 |
| Observed global available immediately after deletion | 4,767,744,000 |
| Observed global available increase during deletion | 1,954,250,752 |

The archive streamed directly from stable source files to avoid a large
uncompressed staging tree while disk space was tight. Archive writes checked
a 2 GiB free-space floor. Per-inode `st_blocks` are nominal allocation and can
include shared overlay/CoW extents. Global available space was observed while
other authorized agents could write elsewhere, so its change is not asserted
to equal the cache's apparent or attributable physical size.

The initial launch rejected an advanced main HEAD before lock acquisition,
cache inventory, archival or deletion. Its original script, failure and exact
Git output are retained under `attempt-1-head-advance/`. The retry accepted
only a clean descendant main checkout and continued to compare historical
proof bytes against the fixed requested parent. It did not relax any cache,
inode, hash, lock or process guard.

The original README and artifact manifest from `0c62da0f` are retained as
`README-original-0c62.md` and `artifact-manifest-original-0c62.json`. This
additive wording correction changes no raw archive or historical qualification.

Run `python3 verify.py .` from this directory for an independent read-only
check of all sealed artifact hashes, full inventory equality, required
preservation coverage, every archive member/hardlink, four process scans and
allocation/free-space records. The original target need not exist. The
executed `reclaim.py` is retained for review and should not be re-executed;
future final-source release builds need a new explicit exclusive cache lease.
