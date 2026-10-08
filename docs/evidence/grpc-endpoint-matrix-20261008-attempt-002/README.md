# Endpoint matrix building

Source `50ac70e5c9818fe870719b37acede99f12710fe9`.

The controller builds and freezes a source-pinned release executable, runs the requested 2,560 functional cells at fixed count 20 with a 60-second cutoff, and captures native N/2N endpoint counters and separate Callgrind profiles. Counter cells cover five endpoint pairs, five shapes, one KiB h2c identity, concurrency one, and three repeats. It retains all failed cells and is configured to publish the outcome directly to main. These are shared-host diagnostics; the resource campaign may run concurrently. Read-all corpora, saturation, cold/idle, task wakeups, syscalls, dedicated-host statistics, and noise qualification remain open.

The controller was alive and its source-pinned release build was running when launch.json was captured. No completed captures are claimed. See started.json for commands and hashes, and the future outcome report for actual elapsed time and results. Overall production and performance qualification remain false.
