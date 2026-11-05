# Borrowed self-recursive validation

This record measures a code generator change for map-free, group-free,
required-free messages whose message fields refer only to themselves. Their
generated validators recurse over bounded borrowed byte slices. Other schemas,
owned-message generation and external mappings retain the existing `Wire`
validator. Existing generated code remains compatible.

The measured baseline is commit `41612b32e6e3466406ff63138a1e2d65c0d5d22e`;
the candidate adds the retained `src/codegen/parse.rs` overlay and proof-only
fixtures. Both use a source snapshot with workspace membership reduced to core.
The benchmark manifests recorded only seven selected source paths, not every
compiler input. A post-build comparison of the retained 168-file work source
with Git found the generator/proof overlay plus changes to `Cargo.toml` and the
workspace `Cargo.lock`. That later inventory does not establish the complete
contents used during the builds. The standalone benchmark lockfile is among
the seven pinned inputs.
The measurements predate the separate attribute-selector changes in the
working tree. They are not measurements of that combined tree.

On November 3, the first local screen used five pairs and the longer
confirmation used seven. A focused nine-pair holdout ran November 4 Chicago
time after an environment restart; its recorded host name differs. Each pair
randomizes baseline/candidate pbrs and Prost processes. The initial screens
used three samples per process; the later runs used five. Samples use process
CPU time for the full parse, selected access and drop.
The input and output checksums must match. The focused holdout used ten times
the initial iterations and one pinned CPU on a shared Linux x86_64 host.

| Focused holdout | Candidate / baseline CPU | Candidate / Prost CPU |
|---|---:|---:|
| Depth-8 parse | 0.798 (0.767–0.808) | 0.609 (0.582–0.646) |
| Depth-64 parse | 0.693 (0.674–0.711) | 0.289 (0.277–0.299) |
| Depth-8 parse and full read | 0.924 (0.897–0.951) | 2.374 (2.309–2.444) |
| Depth-64 parse and full read | 0.934 (0.917–0.962) | 2.014 (1.915–2.051) |
| Mixed parse and full read | 1.004 (0.918–1.034) | 0.936 (0.903–0.955) |
| 4 KiB blob parse and full read | 1.002 (0.998–1.022) | 0.967 (0.955–0.986) |

Values are medians and full ranges of nine paired ratios; lower is faster.
The earlier seven-pair run had a 1.030 median mixed-read ratio, with six pairs
above one; the later nine-pair mixed-read range crosses one and five pairs are
above one. Those controls do not prove absence of a regression. Prost controls
also move across separately compiled binaries. The first-child, empty, mixed,
blob and remaining codec rows are retained in full. No allocation or RSS
measurement, dedicated-host comparison, RPC result or overall ranking follows
from this diagnostic.

An isolated differential harness compared 12,433 valid and malformed inputs
between the old and new generated Node validators using borrowed/shared parse
and two direct-validator modes. The generated fixture check found only the
selected validator body changed; its Prost source was unchanged. The expanded
validated-lazy consumer covers selected and fallback schemas, nested oneofs and
repeated messages, unknown groups, UTF-8, recursion limits, shared inputs,
mutation, clones and getter revalidation. The pinned regeneration check reported
all 13 bundled bindings unchanged. Integrated selector, consumer and strict
consumer-Clippy checks passed after applying the five-file patch to the current
working tree. These finite checks do not prove every possible schema or input.

Run `python3 check.py` from this directory in a checkout containing the named
baseline commit. The checker validates archive hashes, reconstructs the patch
against Git, checks generated fixtures and proof receipts, and recomputes every
paired CPU summary from all 1,224 captured processes. It compares the retained
post-build source inventory with Git; that is not a build-time snapshot. It does not run code
from the archive. Frozen executable hashes and Cargo artifacts are retained,
but executable bytes, toolchains and caches are omitted. The rustc/LLD strings
were observed in ELF comments after the builds; they are not independently
attested. The records do not independently attest the copy from Cargo's build
artifact to the measured binary. Selected source hashes are not a complete
compiler filesystem trace.
