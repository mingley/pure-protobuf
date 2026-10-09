# Response headers during a stalled upload

A valid trailers-only peer rejection could arrive while a unary or server-streaming request waited for flow-control credit. The client did not read those headers until the upload finished or failed, so a peer that kept the request half open made the call return DeadlineExceeded instead of ResourceExhausted.

The retained old diagnostic executable reproduces both failures. It was linked directly against old-source library artifacts and is not a clean-source qualification. The two later reruns used a ten-second watchdog, without finite process limits. Earlier diagnostic negative oracles assumed Internal incorrectly; their original failures are retained and do not count as successful controls.

Source `930b3e17b585542d151b3b2054116c11dd3b3bf7` watches response headers only when the upload waits. A valid HTTP 200 trailers-only non-OK status ends the upload promptly. Ordinary headers are saved until sending completes; OK, malformed or missing status, out-of-range codes and non-200 HTTP cannot turn an incomplete upload into success. Cancellation and deadline checks retain their biased order. Unary response commitment survives upload failure and timeout.

Five new public HTTP/2 tests cover both rejection paths, ordinary headers followed by complete request/response bodies, metadata/trailer retention, and ten invalid/success-header cases. All 569 unit and 89 integration tests passed (one unit ignored), including retry, replay budgets, admission, deadline and final-status controls. Strict all-target Clippy and formatting passed. The exact source patch, fixture, hashes and original outputs are retained. No runtime dependency or unsafe code was added.

The earlier-source optimized resource campaign runs independently on `68484746`. It cannot qualify this later runtime change. The complete performance matrix, broader native regression suite, remaining features and production qualification stay open; no throughput or instruction gain is claimed for this change.

Run `python3 check.py` to verify the inventory.
