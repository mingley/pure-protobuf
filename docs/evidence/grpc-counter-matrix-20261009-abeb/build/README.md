# Frozen benchmark build

Source `abeb7bcf2ee653bff899ba14f505ced9b373f963`. The release build uses allocator counters. Its Cargo-selected executable hash, tool versions, lockfile hashes, command and full build logs are retained in the capsule. The counter driver verifies this record, binary and clean source before and after every group. This build is preparation for measurement; it contains no performance results.

Run `python3 check.py` to verify the archive inventory.
