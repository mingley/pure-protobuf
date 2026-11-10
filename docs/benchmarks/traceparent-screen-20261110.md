# Trace-context extraction screen

This compares the existing parser at
[`cab0e580`](https://github.com/mingley/pure-protobuf/commit/cab0e5803c80c163722b5ea7356327a812510edf)
with the iterator candidate
[`ea4f6e89`](https://github.com/mingley/pure-protobuf/commit/ea4f6e89f63532b117c2f8d541b4d7a4fb01a846).
It measures opt-in OpenTelemetry context extraction, not a complete RPC or a
comparison with tonic. The candidate stops collecting the hyphen-separated
fields into a temporary vector. Parsing behavior is unchanged for the inputs
covered by the tests.

[Run and logs](https://github.com/mingley/pure-protobuf/actions/runs/42804214588) ·
[Raw samples](traceparent-screen-20261110.json) ·
[Reconstruct and run the probe](traceparent-screen-20261110/README.md)

The isolated probe built both sources with the same resolved lockfile and Rust
1.99.1 release settings on a shared Linux AMD EPYC 7763 runner. The baseline
was seeded from the workspace lockfile, then resolved without `--locked`; that
resolved lockfile was copied to the candidate and their hashes matched. The
lockfile itself was not retained, so the artifact alone cannot reconstruct
the exact dependency resolution. The driver set CPU
affinity, then read back the mask `[0]`; child processes inherited that mask.
It measured child process CPU for 10 alternating pairs of 1,000,000 extractions
per case. CPU timing used non-instrumented builds: the counting allocator is
compiled only for the separate 20,000-extraction allocation runs. The direct
carrier isolates propagation; the metadata carrier uses the same
`Metadata::get` access as the server extractor.

| Input | Carrier | Median of within-block CPU ratios (candidate / base) | Candidate faster in paired blocks |
|---|---|---:|---:|
| Valid version 00 | Direct | 0.894 | 10/10 |
| Valid version 00 | Metadata | 0.928 | 10/10 |
| Missing header | Direct | 1.004 | 3/10 |
| Missing header | Metadata | 0.999 | 5/10 |
| Invalid span ID | Direct | 0.719 | 10/10 |
| Invalid span ID | Metadata | 0.880 | 10/10 |
| Future version with extra fields | Direct | 0.713 | 10/10 |
| Valid header with tracestate | Metadata | 0.974 | 9/10 |

The table and verifier recompute the median of the individual block ratios
from the raw time arrays. The driver's `candidate_over_base_median` field
instead divides the two separate time medians, so it can differ from the table.

The missing-header controls overlap, as does one tracestate block. The recorded
affinity mask is not CPU isolation: host contention and throttling can affect
results, and the fixture did not separately monitor each child's affinity.
Short shared-runner samples do not establish end-to-end latency or a general
performance ranking.

For 20,000 valid version-00 extractions, allocation counts changed from
40,001 to 20,001 for the direct carrier, and from 80,001 to 60,001 for the
metadata carrier. Each run counted one additional allocation common to both
versions. This comparison is against the pre-existing metadata lookup; a separate
metadata change must be measured jointly rather than adding these results.

The candidate passed the
[stable and Rust 1.85 trace suite](https://github.com/mingley/pure-protobuf/actions/runs/42803935559),
strict stable lint and docs, and a 12,150-input differential matrix against the
previous parser. That matrix rejected an intentional extra-delimiter mutant.
These checks preserve existing parsing behavior; they do not prove complete
W3C conformance.

The [workflow artifact](https://github.com/mingley/pure-protobuf/actions/runs/42804214588/artifacts/13758341719)
contains the raw report, CPU information and binary/lockfile digests. Its ZIP
SHA-256 is `730444e1476295cde05b6cd87a52dc959059ee4143ce651eabb4b239f72a683`
and it expires on February 8, 2027.
