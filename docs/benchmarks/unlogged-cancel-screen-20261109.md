# Unlogged cancellation primitive, November 9

Two paired process-CPU screens found losses in **unmerged** cancellation
logging drafts. Both measured the private `RpcCancel` primitive with no
logger configured. Filter-excluded calls still use the logging-capable path
in the second draft and were not measured here. These are not RPC latency or
throughput results.

Both use baseline
[`001d0b8b`](https://github.com/mingley/pure-protobuf/commit/001d0b8bc9e89a098047695c3ba73401ba330ea3).
The first candidate uses source blob `41645c0fb12ee0642b1af4f1f9f89444f1d8066d`;
the second uses `2aedeea8dc72f385401af6e9e32d202b088d9e6f`.
The [first run](https://github.com/mingley/pure-protobuf/actions/runs/42713793044)
and [second run](https://github.com/mingley/pure-protobuf/actions/runs/42715528214)
both passed and removed their scratch branches.

| Unlogged scope | Atomic-state draft: median (range) | Capability draft: median (range) |
|---|---:|---:|
| Read an uncancelled flag | 0.994 (0.885–1.060) | 0.991 (0.860–1.023) |
| Construct and drop using the cached flag | 1.002 (0.997–1.010) | 0.994 (0.911–1.057) |
| Repeatedly signal the same flag | **1.496 (1.484–1.507)** | **1.030 (1.004–1.057)** |
| Construct, signal once, check and drop | **1.280 (1.259–1.295)** | **1.022 (1.011–1.053)** |

Numbers are candidate / baseline process-CPU ratios; lower is better. Each
signal and lifecycle pair was slower in both screens. The capability design
keeps the baseline cancellation store and wake path for unlogged calls, but
adds a capability check and changes flag caching. The short read measurements
varied in both directions. The two screens ran separately, so do not compare
their raw times as a controlled A/B between the two drafts.

Both used Rust 1.99.1 release builds on a shared AMD EPYC 9V45 runner, with
the same private API temporarily exposed in both binaries. The candidate
primitive was overlaid on baseline without the candidate unary integration.
The probe calls only the ordinary constructor; it does not exercise
logging-capable construction, and integrated builds may optimize differently.
The baseline binary
SHA-256 was
`d258d74418480aaee2a5604e88e961173686eeeda5241480e87a830703032804`.
The first candidate was
`ab2a472b19d76fcea682c88def1c828c72dddf114cf2223e4aedb86cfc2d7d74`;
the second was
`786eeed62901b96f093dc68bdd86340a2cbadf027faa1e0f3a63201ac4563e33`.

One available CPU was pinned. Each subprocess ran 50,000 warmups before
2,000,000 measured operations; both binaries also ran one process warmup per
case. Ten pairs alternated baseline-first and candidate-first. CPU time was
measured inside each process; wall time is retained separately. The signal
row repeatedly cancels one flag without waiters. The lifecycle row uses the
thread-local spare, signals once, reads the flag and drops it. Construction
also uses the warmed spare; cold allocations are not measured. The flag-read
row was very short, making it particularly noisy.

Logging, asynchronous scheduling, contention, HTTP/2 and handlers are absent.
The runner is shared and frequency is uncontrolled. These descriptive pairs do
not quantify application-level impact or establish a dedicated-host ranking.

The [retained data](unlogged-cancel-screen-20261109.json) is reconstructed
from the validated run logs and includes every timing and pair order. The first
[original artifact](https://github.com/mingley/pure-protobuf/actions/runs/42713793044/artifacts/13717336804)
has SHA-256 `763d5f2fcab8b26f32c580a8cb1a40944ecc426fd20fda000d135bbc0e42fccd`.
The second [original artifact](https://github.com/mingley/pure-protobuf/actions/runs/42715528214/artifacts/13717618076)
has SHA-256 `cb668a064928f5d7c48d377c3b4da505e3e17714223f2d5cd27dcb56ca2dc1c8`.
Both expire November 23, 2026.
