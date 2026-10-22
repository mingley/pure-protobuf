# Streaming resource soak, 2026-10-22

| Check | Result |
|---|---|
| Requested duration | 24 hours |
| Recorded duration | 1,922.73 seconds (0.53 hours) |
| Day status | failed |
| Resource checks | failed |
| Process exit code | 101 |
| Production qualified | No |

Source: `6cd91911afce24cceee6df3f60a945453e5b04da`.

This is a shared-host resource campaign with the original limits and checks.
Other feature, release and comparative performance gates remain separate.

Reported failures:

- incomplete, duplicated or reordered cycle phases
- independent samples do not cover the requested duration
- missing or incomplete requested duration
- resource test child failed

The archive retains events, process samples, logs, command records, the preview
prerequisite and the pinned source controller. Executables are represented by
their hashes. A missing or incomplete report remains an incomplete result.

Verify every archived member:

```sh
python3 docs/evidence/grpc-streaming-soak-20261022/check.py
```
