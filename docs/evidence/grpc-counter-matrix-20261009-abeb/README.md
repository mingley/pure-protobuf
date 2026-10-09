# Endpoint counter queue

Source `abeb7bcf2ee653bff899ba14f505ced9b373f963`. At this snapshot no captures have run. Each native, syscall and Callgrind phase schedules 7,200 N/2N captures across four payload sizes, h2c/TLS, identity/gzip, three connection/concurrency levels, five client/server pairings, five call shapes and three repeats. RPC deadlines and the outer watchdog remain unchanged.

The queue waits for resource attempt 009 to finish or its prerequisite gate to stop. It requires the verified benchmark; failed resource qualification does not suppress diagnostic counter measurements. No profiles or compilations run during the actual resource day. Each terminal phase publishes all raw captures, failures and per-group comparison ledgers directly to main, with split archives and standalone integrity checks.

These are shared-host synthetic steady-state counters. Read-all corpora, saturation, cold/idle lifecycle, task wakeups, measured noise and dedicated x86_64/arm64 timings remain open. These scheduled captures do not establish performance dominance or production readiness.
