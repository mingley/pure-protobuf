# Boxed service dispatch

Generated services select the method before boxing its future in a Router. This reduces the size of the allocation used for dispatch. The ordinary Service::call path and hand-written services retain their existing behavior. The hook is additive; dispatch uses no new unsafe code or dependencies.

The benchmark is clean source `f8517b53fefaa70866f19ec4896d46254f812b4e`, release binary `a43c6821c86e2a17422ad0694206364fef5eddf8325e0340d17fde6ea556d440`. The build, locked dependency pins, exact drivers, raw captures, failed capture, and ledgers are retained. Later documentation and hook tests were checked on `1f43c24dca8f1ef9123a986b2932659d11e660ad`; this does not relabel the benchmark source.

All 2,560 functional cells passed: all sixteen native/tonic and pbrs/prost endpoint pairings, five shapes, four sizes, h2c/TLS, identity/gzip, and loads 1:1 and 1:16. These N=20 runs verify completion, not saturation performance. Native endpoint counters passed 150/150 captures; instruction captures passed 149/150. One tonic/prost client to native/prost bidi N=400 capture completed 397/400 requests and timed out on three; the original failure remains in the ledger.

Against the earlier `50ac70e5` capture, generated native/pbrs server instruction counts fell roughly 1–5% in the five 1 KiB h2c identity concurrency-one shapes (repeat zero). They still exceed the matched tonic/prost server counts. Manual native/prost benchmark services use the default hook and do not receive the generated specialization. Shared-host measurements, concurrent compilation, setup costs, and missing noise estimates prevent claims about latency, CPU leadership, or general dominance.

Two hook tests, strict all-target Clippy, and strict API docs passed on the documentation/test commit. Broader native all-feature regressions are running sequentially; their initial aggregate build was interrupted to avoid disk exhaustion. Earlier disk and test-helper build failures are retained. No 24-hour soak or complete performance or production qualification is claimed.

Run `python3 check.py` to verify the raw inventory. See summary.json and the per-side ledgers for wins, losses, unavailable metrics, and failed captures.
