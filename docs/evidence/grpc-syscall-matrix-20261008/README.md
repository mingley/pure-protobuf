# Source-pinned syscall measurements

The benchmark was built from clean source `50ac70e5c9818fe870719b37acede99f12710fe9`; the capture driver is clean source `aced76eacf5469a9c98a8370524b77fa70ebcabc`. The build record, lock pins, exact driver, controller, raw endpoint traces and drift checks are retained. A newer driver does not relabel the benchmark binary.

All 150 captures passed at N=200 and 2N=400. They cover five endpoint pairings, five RPC shapes, three repeats, one KiB h2c identity, and concurrency one. Linux strace counts calls and errors across all endpoint threads until exit. Setup is included. Ptracing changes timing and scheduling, and an optimized resource build ran concurrently; these measurements establish counter observations, not latency or CPU leadership.

The per-side ledger includes syscall and allocation differences against a matched tonic/prost peer. See summary.json for wins, losses and missing metrics. TLS, gzip, other payloads and loads, task wakes, read-all corpora, measured noise, and dedicated-host statistics remain open. Overall qualification is false.

Run `python3 check.py` to verify the raw inventory.
