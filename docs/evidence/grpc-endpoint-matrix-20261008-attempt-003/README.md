# Endpoint matrix outcome

Source `50ac70e5c9818fe870719b37acede99f12710fe9`. The raw capsule retains the source-pinned release build, functional cells, native N/2N endpoint counters, separate Callgrind captures, and all failed cells. Overall qualification remains false.

Of 2,560 functional cells, 2,559 passed; the tonic/pbrs compressed 1 MiB bidi control missed its 5-second deadline. All 150 native counter captures passed. Of 150 Callgrind captures, 149 passed; a tonic/prost server-streaming reference failed, so dependent ledger rows are marked failed_capture. The native ledger has 98 wins, 79 losses, 2 ties and 181 missing rows. The Callgrind ledger has 65 wins, 96 losses, 7 ties, 168 missing rows and 24 failed rows. These are diagnostic comparisons, with no measured noise bound. Source-pinned syscall observations are recorded separately in ../grpc-syscall-matrix-20261008/.

The requested functional matrix covers 2,560 cells at 20 RPCs per cell, with a 60-second cutoff. This checks wiring and completion, not saturation. The N/2N captures cover five endpoint pairings, five shapes, one KiB h2c identity, concurrency one, and three repeats. Read-all corpora, cold/idle, higher loads, task wakeups, syscalls, dedicated-host statistics, and measured noise bounds remain open. See outcome.json and archived report/ledger files for actual results; a requested command is not a successful measurement.

Run `python3 check.py` to verify every retained file.
