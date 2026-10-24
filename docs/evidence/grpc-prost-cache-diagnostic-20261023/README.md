# Prost cache diagnostic, 2026-10-23

| Check | Result |
|---|---|
| Requested duration | 24 hours |
| Recorded duration | 86,400.80 seconds (24.00 hours) |
| Diagnostic status | passed |
| Completed fixture cycles | 26679 |
| Child exit code | 0 |
| Production qualified | No |

Runtime source: `beb4ecd9a9b2143c440892607d5b486318062b61`.

This shared-host diagnostic repeats the independent TCP/HTTP2 compressed-decode
fixture with Prost and retained-cache counters. Each cycle repeats the same
fixture; cycles are not additional distinct production tests. Preview completion
does not satisfy the requested day. This is separate from the C8 and C11 pbrs
resource campaigns and does not establish production readiness or broad speed
superiority. The archived provenance identifies the diagnostic test-only overlay,
unchanged runtime files, build profile, compiler tools and executable hashes.

Reported failures:

- None reported.

The archive contains immutable pack-time copies of the day and preview records,
fixture/controllers, samples, progress and command/provenance records. ELF bytes
are omitted; their hashes and sizes are retained in publication-snapshot.json.

Verify archived members:

```sh
python3 docs/evidence/grpc-prost-cache-diagnostic-20261023/check.py
```
