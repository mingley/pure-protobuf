# Remaining historical verify-main cache preservation

The explicitly approved, completed `verify-main` cache was preserved and retired
in two stages. Every remaining regular payload was hashed and archived, including
all 353 ELF paths, objects/shared libraries, fingerprints, full build outputs,
generated source, dependency information, rlibs, fixtures and manifests. There
are no hash-only payload exclusions. Directory-relative, no-follow removal was
qualified against freshly verified directory/file inode identities only after
streamed archive content/mode/ownership/link checks passed.

| Stage | Regular paths | Logical bytes, aliases included | Unique regular bytes | Local archive bytes |
| --- | ---: | ---: | ---: | ---: |
| `target/shared-consumers` | 559 | 349817411 | 343335931 | 83062172 |
| Remaining `target` | 9579 | 5506588470 | 4909642678 | 1022370796 |

The clean detached source context was
`de0bcdf2dd55fa7f436139813a87b967187c8f5d`. This context does not attribute every
historical cached artifact or qualify current main. Actual older logs, including
the original retry/deadline failure, shared-consumer results and prior prune
histories, remain preserved with their original labels. The 35 native and 20
other previously pruned ELF paths remain unavailable; this operation cannot
recreate them. Only payloads remaining at the new inventory are archived.

External cache symlinks, including the shared conformance-build/protoc targets,
were archived as link metadata and unlinked only within the retired namespace.
Their targets were never followed, copied, mutated or deleted. Source worktrees,
shared SDK/registry data, immutable binaries and older proof files were excluded
from retirement.

Both stages used frozen helper
`cbbec635627145605d11bcc3a116a3eca9ec5908134b818d0d85c17e45542c91`.
Each discovered Cargo lock held a dedicated flock plus Linux OFD write lock.
The Linux x86_64 ABI/constant fallback was source-checked against retained system
headers and AST-compared to the independently reviewed helper. A tiny fixture
passed 30 independent contender observations across hash/archive/verification
and anchor closure. The original fixture failed closed when Python omitted the
named OFD constant; its raw failure and old helper versions remain. Earlier
classic POSIX-lock lifetime limits are documented in the immutable
[prior retirement proof](../cache-retirement-20261003/README.md); these new
operations use the separately verified OFD primitives.

The smallest sampled free space was 2,249,887,744 bytes, above the unchanged
2,147,483,648-byte reserve. Every archive write separately checked available
space. The summed RX-owned cache cap remained the same fixed value. Historical
root-cache allocations are measured separately from that RX-owned sum; archives
consume global disk as proof artifacts. Point-in-time process visibility denials
are retained, without a claim of universal process visibility. No Cargo,
compiler, performance or shipping gate ran during this housekeeping operation.

`records.tar.gz` contains 194 hashed members: frozen helper/authorization/ABI
sources, fixture attempts, historical logs and limitations, complete inventories,
source/tool contexts, held-lock and process observations, sampled resources,
stream-verification/fresh-check/removal results. `index.json` indexes both large
local archives by exact path/hash. Default metadata validation passed; this is
integrity evidence and does not recreate missing historical provenance.

The portable read-only checker does not extract archives, execute helpers,
launch compilers or delete files. Its default invocation explicitly does not
reread the large local archives:

```sh
python3 docs/evidence/verify-cache-retirement-20261003/check.py
```

To also stream-verify all 10,138 regular payload paths and 12,407 members, with
exact hashes, modes, ownership and hardlink/symlink topology:

```sh
python3 docs/evidence/verify-cache-retirement-20261003/check.py --payload-root \
  /workspace/pure-protobuf/work/cache-retirement/verify-main-approved-20261003T070055037875Z
```
