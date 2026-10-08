# Peer rejection after a closed upload

The optimized resource preview reported Internal instead of ResourceExhausted on overload. A test-only diagnostic replay reproduced it: the backend send error was User(InactiveStreamId), without transport retry evidence. The raw old-source replay remains a diagnostic, not a clean-source qualification.

A deterministic native HTTP/2 regression flushes a trailers-only rejection before resetting the request half, then obtains an actual closed-stream send error. Before the fix, the regression fails with Internal instead of ResourceExhausted. Earlier test compilation and fixture-drive errors are retained separately.

The client now inspects already-ready response headers after a backend send error, even when that error carries no transport retry evidence. A valid HTTP 200 trailers-only peer rejection takes precedence. OK, missing or malformed status, non-200 HTTP, and absent queued headers retain the original upload error. Local errors never wait for response headers. Application rejection and REFUSED_STREAM retain their existing commitment and retry rules; cancellation and deadlines still use the existing outer race.

Runtime fix `e2ee3ada`; validation source `684847461083d1a7d680031f02b05fabb17ab59b`. All 569 unit tests passed (one ignored), as did 84 retry, policy, admission/deadline, and final-status integration tests. Strict all-target Clippy, formatting and API docs passed. No new runtime dependencies or unsafe code were added. The earlier generated-documentation edit was checked against twenty actual generated files: contents are identical after removing standalone doc-comment lines.

The broader frozen dispatch regression run passed six targets, then stopped at the isolated rustls consumer because its lockfile omitted seven local dependency entries. The corrected lock retains all existing package versions; its locked metadata now resolves. Actual consumer execution and the remaining broader suite are separate follow-ups.

A peer that rejects while leaving a flow-control-stalled upload open can still make the call wait until its own deadline. This separate response race is not closed by the queued-error fix. The previous day and optimized preview failures remain failures. Production readiness, the full feature set and performance dominance remain open.

Run `python3 check.py` to verify the retained inventory.
