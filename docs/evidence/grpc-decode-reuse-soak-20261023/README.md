# Decode reuse resource soak, 2026-10-23

| Check | Result |
|---|---|
| Requested duration | 24 hours |
| Recorded duration | 86,405.46 seconds (24.00 hours) |
| Day status | completed |
| Resource checks | passed |
| Process exit code | 0 |
| Production qualified | No |

Source: `beb4ecd9a9b2143c440892607d5b486318062b61`.

This is a shared-host resource campaign with the original limits and checks.
The unchanged resource fixture uses pbrs decoding. It does not exercise the new
Prost decoded workspace and cannot qualify that cache for 24 hours. Independent
cache lifecycle tests and sanitizer checks are separate evidence. Other feature,
release and comparative performance gates remain separate.

Reported failures:

- None reported.

The archive retains events, process samples, logs, command records, the preview
prerequisite and the pinned source controller. Executables are represented by
their hashes. A missing or incomplete report remains an incomplete result.

Verify every archived member:

```sh
python3 docs/evidence/grpc-decode-reuse-soak-20261023/check.py
```
