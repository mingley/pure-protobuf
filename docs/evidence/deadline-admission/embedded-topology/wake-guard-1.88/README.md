# Observable task waker compatibility fixture

The actual Rust 1.88 final caller Clippy gate at source 98a43c55 failed because
`clippy::manual_noop_waker` is unknown under the imported test warning policy.
The complete red log is retained here. This test-only overlay removes that
attribute and makes the unique Arc-owned WakeGuard increment an independently
observed AtomicUsize. Both cancellation fixtures explicitly exercise one wake
and confirm that canceled registrations receive no further wake after capacity
updates. Existing weak-release, request-drop, stream-ID, capacity assertions and
clock bounds remain intact. The shared helper's constructor is updated in its
additional mixed-legacy fixture too.

The runtime backend, native callers, unsafe policy and imported-source warning
policy do not change. Before/after source bytes are retained here; the packaged
`wake-guard-1.88-overlay.json`, exact patch and read-only replay record both source
hashes and the original topology mapping hash. The original 604f source/package
proof remains immutable. GN13 will run the actual Rust 1.88 strict and unit gates
on the resulting caller source; these fixture bytes have no compilation claim
from this overlay preparation.
