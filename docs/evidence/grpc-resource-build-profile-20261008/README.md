# Explicit resource build profiles

The campaign runner supports an explicit debug or release test build. Debug remains the default for existing commands. Production-artifact campaigns select `--build-profile=release`. Schema v5 records the requested profile and Cargo metadata for the exact selected executable. It rejects a command/profile mismatch, unoptimized release test, incorrect assertion mode, missing/duplicate artifact metadata, or changed source. Profile mismatches stop before launching the child. Scenario limits, payloads, deadlines, recovery thresholds, and elapsed-day checks are unchanged.

All 18 Python evidence guards pass. The same validator still accepts the retained schema-v4 preview and rejects its failed day. The capsule retains these outputs, original test iterations, source hashes, and the working-tree patch. This record adds build attribution; it does not contain a completed optimized campaign.

The prior debug executable failed after 705.18 seconds at the 1 MiB TLS identity bidi case against a 3-second deadline. Its original failure remains published. Debug latency diagnosis, optimized production-artifact qualification, complete features, and comparative performance remain open.

Run `python3 check.py` to verify the retained inventory.
