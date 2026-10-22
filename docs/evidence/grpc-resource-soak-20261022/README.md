# gRPC resource campaign, started 2026-10-22

Source: `8fe0f8aeb41356ff427f44542947f36f6d2a2dae`. Requested duration: 86,400 seconds.
Recorded duration: 17748.395289456003 seconds.
Process exit code: 101.
Resource checks: `failed`. Day disposition: `failed`.

The native test run at launch still recorded three failures. This resource
campaign is independent of those functional blockers.

The child panicked at `tests/common/mod.rs:1118`: a raw-peer `RST_STREAM(CANCEL)`
probe expected `CANCELLED` or `UNAVAILABLE` and observed `Ok(())`. It stopped in
cycle 5,419 after 4 h 55 min 48 s. Resource recovery had completed before the
failing probe; this run does not demonstrate a resource leak.

This is a shared-host resource result. It does not establish production readiness
or superiority over tonic/prost. The original limits and validation checks remain
in force. Failures: `["incomplete, duplicated or reordered cycle phases", "independent samples do not cover the requested duration", "missing or incomplete requested duration", "resource test child failed"]`.

Run `python3 docs/evidence/grpc-resource-soak-20261022/check.py` to verify every
retained member. The archive contains the command, events, process samples,
validation report and logs. Executables are represented by their hashes.
