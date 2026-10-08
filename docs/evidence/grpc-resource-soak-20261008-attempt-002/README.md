# Second resource campaign

Requested: a 30-second preview, then an actual 86,400-second run.
Source: `d659c2ff9a9186ad76da092ea407ad784ba07fe9`.

The detached controller was launched on 2026-10-08. Its initial phase is building the frozen preview executable. This start record is not a completed soak or a production qualification. `started.json` records the source, tree, seed, exact command, and controller hash.

The controller retains the preview and day outcomes and pushes reports, raw event/process logs, validation results, and file checksums directly to main. It also publishes a failed attempt if either phase fails. This directory will contain the final outcome when publication succeeds. The environment must keep running for the whole campaign.

The fixture covers native plaintext/TLS, identity/gzip, exact payloads, paused readers, overload, cancellation, deadlines, TCP faults, and drain. It does not close the remaining performance, feature, allocator, kernel-memory, or dedicated-host qualification requirements. See [the measurements and retained failures](../grpc-readiness-20261008/README.md).
