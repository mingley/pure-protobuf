# Queued endpoint counter sweeps

Source `a9fe4c2d`, with its benchmark built before resource attempt 008. At this snapshot no sweep captures have run. Native allocation/context-switch, syscall and Callgrind phases are queued after the resource campaign ends, including a failed resource outcome. A prerequisite failure before the resource campaign starts stops the queue and records the error.

Each phase schedules 7,200 captures: empty, 1 KiB, 64 KiB and 1 MiB payloads; h2c and TLS; identity and gzip; loads of 1 connection/1 total in-flight call, 1/16 and 64/1024; five client/server pairings, five RPC shapes, three repeats, and N/2N runs. N is at least the total in-flight count. Original RPC deadlines remain unchanged. Failed cells are retained and the other profiles continue.

Each terminal phase publishes its raw captures and per-group loss ledgers directly to main under `native`, `syscalls` or `callgrind`. Archives are split into parts with at most 32 MiB of logical file content. Their manifests record every file and hash; each phase includes a standalone checker. Publication errors retain the local commit and captures. The queued controller and its hashes describe the scheduled work, not a completed measurement.

These sweeps cover synthetic steady-state counters. Read-all corpora, saturation, cold/idle lifecycle, task wakeups, measured scheduling noise and dedicated x86_64/arm64 timings remain open. Production qualification and performance dominance over tonic remain false.

The queue has now stopped at a failed native regression prerequisite. No campaign captures started. See [the failure record](../grpc-resource-admission-20261009/README.md) and `queue-outcome.json`.
