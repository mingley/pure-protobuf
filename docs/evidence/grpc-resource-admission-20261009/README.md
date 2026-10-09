# Mixed resource fairness failure

The native continuation on source `a9fe4c2d` passes 19 integration targets, then fails one of 25 resource-bound tests. The mixed bulk/slow-peer/reset-storm test completes 31 of 32 small RPCs and receives one ResourceExhausted rejection; none times out. The original workload, ceilings, deadlines and assertions remain unchanged.

Thirty isolated repetitions of that same Cargo-selected executable pass 28 times and fail twice. Both isolated failures report only 14 bulk messages arriving during the small-RPC measurement window, below the required 16. The original rejection and these progress failures are separate observations; their causes are not established by this evidence.

Resource attempt 008 stops at its regression prerequisite. Its preview and actual 24-hour run never start. The counter queue also stops before collecting any captures. Remaining native targets, including the isolated caller-rustls consumer, have not run in this continuation. The archive retains the source pins, commands, artifact hashes, all outcomes and queue errors; executable bytes remain local. Run `python3 check.py` to verify its inventory.

Production readiness, remaining features and performance dominance over tonic remain open.
