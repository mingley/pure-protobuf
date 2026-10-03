# Completed cache preservation, 2026-10-03

Three explicitly approved historical cache namespaces were preserved, verified,
and retired. All 7,432 remaining regular paths were hashed. Archives retain 7,386
regular paths, including all 1,161 ELF paths (objects and shared libraries count);
46 semver incremental/nonselected paths have an explicit hash-only disposition.
The verify-main target, source worktrees, shared SDK/registry/protoc/conformance
builds, immutable work/bin copies and existing evidence were outside the scope.

| Namespace | Regular paths hashed / archived | Logical namespace bytes, aliases included | Unique regular bytes | Local archive bytes |
| --- | ---: | ---: | ---: | ---: |
| root `target/semver-checks` | 4882 /4836 | 2648157280 | 2339638866 | 413537353 |
| `work/gn13/target` | 1175 /1175 | 600849338 | 541132467 | 165210167 |
| `deadline-admission-native/target` | 1375 /1375 | 797332236 | 731435468 | 213079964 |

Each stage held every discovered Cargo lock with nonblocking exclusive flock and
fcntl locks, recorded visible process users/denials, hashed all remaining paths,
and rechecked the complete path/inode/mode/content set immediately before
removal. Compressed archive creation checked global free space before and after
each write. Streamed readback verified every retained byte/hash, mode, ownership,
and hardlink/symlink relation. Removal used the already verified directory inode
and directory-relative no-follow operations. The preserved archive hash was
checked again after removal. Full fingerprints, build outputs, deps/rlibs/rmeta,
generated source, manifests and JSON are retained under the selected/full scopes.

The smallest observed free-space sample was 2,570,031,104 bytes, above the unchanged
2,147,483,648-byte reserve. The greatest observed summed RX-owned cache size was
1,398,183,047 bytes, below the same fixed cap. Semver's pre-existing allocation
was measured separately as 2,352,930,816 bytes; those root artifacts were outside
the RX-owned cache sum. Large preserved archives consume global disk but are
proof artifacts, rather than compiled cache. Free space at the last retirement
was 5,825,449,984 bytes. These are sampled observations, not claims about unsampled
peaks or a guaranteed future allowance for another build.

Two invalid helper attempts remain distinct. First, hashing the invoked Python
symlink as a regular cache file failed before cache scan/archive/deletion. The
second attempt failed on a tracked source `.proto` symlink after locks/source
preparation and before archive/deletion. The corrected helper treats tool and
source symlinks separately while retaining the strict regular-cache lstat/fstat
checks. Exact v1/v2/v3 helper bytes, first raw stderr/stdout/exits, the second
record, focused symlink fixtures/replay and four independent tiny-cache checks
are included. The first preflight failed before recording its environment; that
missing observation is not reconstructed. Successful retirement uses v3
`bcce5e8ddac81f3563eaab0f4642bf07fe1448877817c4539a2982f24bc14e0b`.

Historical identities remain historical. The three semver terminal logs each
say 196 pass/58 skip/no API update, but do not bind the historical provider commit.
The clean moving root context was `b2974e5e`; it is not a new semver qualification.
GN13's `1b050a3c` context does not attribute every artifact in its mixed consumer
cache. The remaining native `interop188` directory was actually built with
Rust 1.85 at `cc4db053`; stable normalized packaging used `d6c4ddde`. Source and
tool labels from those raw captures are retained. No compiler, Cargo, performance
or shipping gate ran during this housekeeping operation.

The old prune histories list 35 native and 20 other previously removed ELF paths.
They remain unavailable; this proof preserves only the artifacts that remained
at the new inventory, and the older hash records. Hash-only records do not
recreate either those ELFs or the 46 excluded semver payloads. Root-owned
dockerd/containerd fd/maps visibility denials are retained; no claim of universal
process visibility is made. Zombie exclusions, point-in-time process/lock checks,
and the source/ELF attribution limits are explicit in the raw records.

`records.tar.gz` is a self-contained small metadata capsule. `index.json` hashes
all capsule members, inventories the local large archives and gives their exact
paths/hashes. Run the read-only metadata checker from any checkout:

```sh
python3 docs/evidence/cache-retirement-20261003/check.py
```

That check verifies the capsule and the recorded invariants. It explicitly
reports that it has not reread the locally retained large payloads. To also
stream-verify all large payload content/modes/ownership/link topology, use:

```sh
python3 docs/evidence/cache-retirement-20261003/check.py \
  --payload-root /workspace/pure-protobuf/work/cache-retirement/gn13-approved-20261003T052944270800Z
```

The checker never extracts an archive, executes an included helper, deletes a
file, or launches a compiler. The default metadata audit passed 131 members. Root
also independently reread all 5,798 semver archive members before handoff; its
separate result is retained in the capsule. Large archives remain local and are
not added as Git blobs. This preserves reviewable housekeeping evidence without
claiming full reconstruction of older qualification campaigns.

A subsequent independent root invocation reread all three large archives and
passed all 7,386 retained regular paths and 8,980 total members. The command,
checker/capsule/archive hashes, results and historical limits are retained in
`root-payload-audit.json`. This is preservation integrity evidence, not a new
shipping or historical compiler qualification.
