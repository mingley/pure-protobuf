# Adapter send-buffer checks

The original adapter test expected an eight-byte send buffer to fail before dispatch. The adapter supports that buffer, so the test failed after dispatch with no initial error status. The corrected test retains the unsupported-policy checks and separately sends a 1 KiB framed message through an eight-byte buffer and a one-byte peer window. It checks the bytes, initial metadata, terminal status, terminal metadata and dispatch count with compression acceptance enabled and disabled. No runtime code changed.

The all-feature adapter suite and its strict Clippy check are retained in the raw capsule. The original failure remains in the [complete native inventory](../grpc-native-abeb-20261009/README.md). Runtime sources match `abeb7bcf2ee653bff899ba14f505ced9b373f963`; the test source hash and selected executable hash identify the correction.

The resource prerequisite job ran out of disk while generating documentation, before starting the preview or day. Its original status, failed documentation output and the counter job that stopped without a verified benchmark remain in this capsule. The resumed queue clears inactive debug caches before retrying documentation and retains successful prior checks. The admission fairness failure remains open; subsequent resource and counter runs are diagnostic. Production qualification remains false.

Run `python3 check.py` to verify the archive inventory.
