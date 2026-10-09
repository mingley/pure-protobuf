# Complete native test inventory

Source `abeb7bcf2ee653bff899ba14f505ced9b373f963`. The controller attempts all 58 native library/integration targets with all features and one independent test case at a time. 58 targets completed; 2 native/MSRV steps failed. See summary.json and the original output for each failure. No case or deadline was removed or relaxed.

The actual Rust 1.85 default-feature unit and stalled-upload checks run first. Source cleanliness and Cargo-selected executable hashes are recorded. Passed executable caches are removed; failed executables remain local. The portable archive retains commands, source pins, artifacts, stdout, stderr and every completed result. Run `python3 check.py` to verify its inventory.

A test pass does not close remaining features, resource qualification or performance comparisons. The subsequent preview/day and endpoint counter captures are separate diagnostics; overall production qualification remains false.
