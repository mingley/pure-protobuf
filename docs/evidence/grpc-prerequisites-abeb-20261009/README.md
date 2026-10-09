# Current-source prerequisite checks

Source `abeb7bcf2ee653bff899ba14f505ced9b373f963`. Formatting, strict all-target/all-feature Clippy for pbrs and pbrs-grpc, 44 Python campaign contracts and all-feature Rustdoc with warnings denied passed. The isolated caller-rustls consumer ran all eight tests successfully; its Cargo-selected executable and hash are recorded.

The first documentation attempt ran out of disk. Its failed output stays in the archive alongside the successful retry. Inactive debug caches were removed after compiler children exited; failed test executables and raw reports were preserved.

The [complete native inventory](../grpc-native-abeb-20261009/README.md) retains the mixed-load bulk-progress failure and the adapter test's stale buffer expectation. The [adapter correction](../grpc-adapter-buffer-20261009/README.md) separately passes 19 tests on identical runtime sources. These checks do not close the resource run, feature gaps or client/server performance comparisons. Production qualification remains false.

Run `python3 check.py` to verify the archive inventory.
