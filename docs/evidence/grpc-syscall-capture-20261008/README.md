# Endpoint syscall capture checks

The Linux runner now captures syscall counts with strace for each endpoint, including exited worker threads. Calls that return errors are included. It rejects missing, duplicated, negative, nonfinite, and inconsistent summaries. Partial mode also rejects driver drift instead of converting it into a valid counter row. The ledger independently checks scope and per-syscall sums, and rejects mismatched tracer or driver settings between N/2N captures.

The `-D -f -c` probe confirms that the child PID still names the benchmark process and that SIGUSR1 reaches its allocator snapshot handler. All 25 one-KiB h2c identity plumbing cells passed at 20 RPCs per cell, covering five endpoint pairs and five call shapes. These use the prebuilt historical ff65478c executable and remain source_verified=false; they are not current-source performance measurements. Ten smoke guards, ten ledger guards, and six build-record guards pass. An intentional attempt to bind the old build to the clean 50ac70e5 checkout was rejected.

`--source-checkout` permits a newer driver to validate an existing frozen build without relabeling its source. Reports retain the benchmark pin separately from the driver commit, dirty state, exact script, and hash. Both are checked for drift. Raw data and the working-tree patch identify the code tested here.

Ptrace changes timing and scheduling, and tracer CPU is excluded from endpoint CPU windows. Run these captures separately from native context-switch and Callgrind measurements. No native timing, noise-band, complete matrix, beat-tonic, or production qualification is established. Task wakeup capture and other qualification work remain open.

Run `python3 check.py` to verify the complete raw inventory.
