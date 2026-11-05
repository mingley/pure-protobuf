# Rejected borrowed-map lookup candidates, November 3

Three isolated candidates removed the temporary owned key from indexed string
lookups. None was selected: the final candidate repeatedly slowed a small
duplicate-key lookup. The shipped map implementation was not changed.

The experiments start from commit
[`41612b32e6e3466406ff63138a1e2d65c0d5d22e`](https://github.com/mingley/pure-protobuf/commit/41612b32e6e3466406ff63138a1e2d65c0d5d22e).
Each candidate searches the existing sorted index using borrowed bytes for
string keys. The fixtures check last-wins behavior for their tested cases;
the rejected patches did not receive complete correctness qualification.
The variants differ in where they outline the indexed search:

1. Inline the borrowed search.
2. Outline every indexed search, including integer keys.
3. Outline only string searches; retain the inline numeric fallback.

| Candidate and holdout | Median new / old CPU | All-pair range | Decision |
|---|---:|---:|---|
| 1: 8-entry ProtoString duplicate hit | 1.052 | 1.034–1.075 | Reject |
| 2: 256-entry integer distinct hit | 1.030 | 1.001–1.059 | Tentative; possible overlap with a documentation test |
| 3: 8-entry LazyStr duplicate hit | 1.052 | 1.026–1.064 | Reject |
| 3: 256-entry LazyStr, 96-byte distinct hit | 0.861 | 0.806–0.873 | Faster in this lookup screen |
| 3: 256-entry ProtoString, 96-byte distinct hit | 0.910 | 0.735–0.972 | Faster in this lookup screen |

Each holdout uses nine randomized process pairs and three million lookups per
process. Lower ratios are faster. The 8-entry duplicate fixtures have two
distinct keys. Their reads use the existing linear scan, so compiler code
generation can affect that path even though the source change targets indexed
lookups. Assembly inspection of candidate 1 found an additional query-pointer
stack reload in its small ProtoString scan; this observation does not prove
the cause of the measured difference.

The 76-case candidate 3 screen also measures short and long keys, distinct and
duplicate entries, hits and misses, and integer controls. Separate instrumented
processes counted 4,096 allocations and 393,216 requested bytes for 4,096
indexed 96-byte string lookups in the baseline, versus zero for candidate 3.
That accounting excludes map construction and index creation. Allocation
instrumentation is absent from the timed binaries.

Measurements ran on one pinned CPU of a shared Linux host. CPU time is
measured inside the warm lookup loop; the input map, queries and lazy index
are prepared beforehand. Five-pair screens and seven-pair confirmations are
retained alongside the nine-pair holdouts. The candidate 2 holdout and its
full grid may overlap the end of a documentation test; neither is credited
as a quiet result. HashMap is included as a separate-process reference for
Prost's usual map storage, not a Prost decoding benchmark. These data do not
establish end-to-end application or gRPC speed, complete compatibility, or
an overall performance ranking.

## Verify the record

Run `python3 check.py` from this directory in a checkout containing the base
commit. The checker reads the archive without executing its contents. It
reconstructs 72 selected committed core inputs from Git, checks both source
overlays for each candidate, verifies the benchmark, Cargo lockfile, compiler
version, build records and recorded binary hashes, and independently checks
every raw sample, query fingerprint, checksum, pair ratio and allocation
count against the nine summaries.

The archive contains all 1,845 timing pairs and 456 separate allocation
observations. It includes the exact harness and scripts, candidate source
files, build logs and raw results. Executables and toolchain binaries are not
retained, so their recorded hashes cannot be recomputed by the checker. The
toolchain version is recorded, not independently attested, and selected source
hashes are not a complete filesystem-read trace.
