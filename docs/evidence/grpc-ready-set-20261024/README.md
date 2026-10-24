# Ready-set experiment: adoption deferred

The recheck passes its scheduling control, but has **no consistent measured CPU
benefit**. It remains experimental. The qualified runtime stays at `933bf877`;
`daa7eb40` is the isolated candidate.

## What changed

After protocol input returns Pending, poll newly ready RPCs once before parking.
Keep the existing drain priority. A gated independent HTTP/2 peer proves that one
parent poll reads complete DATA and runs the target handler; an unwoken sibling
stays asleep. Valid status/body and same-connection recovery pass before detection.

The baseline fails that final assertion. Both candidate variants pass **87 focused
checks**; the final variant also passes formatting. Parent wakes rise from one to
two in the fixture. Same-turn progress does not prove fewer notifications or speed.

## Release-build CPU

**Microseconds/call, lower is better.** Identity 1 KiB, one connection / one call
in flight, two repeats, counters and profilers off. Raw process CPU at 10,000 calls:

| Native server | Repeat | Baseline | Candidate | Change |
|---|---:|---:|---:|---:|
| Unary | 1 | 47 µs | 49 µs | +4.26% |
| Unary | 2 | 49 µs | 43 µs | −12.24% |
| Bidi | 1 | 108 µs | 108 µs | 0.00% |
| Bidi | 2 | 149 µs | 162 µs | +8.72% |

The campaign completes **48 captures / 360,000 successful RPCs**, zero failures.
Unary-server N/2N slopes are 32→54 and 62→44 µs/RPC; bidi slopes are 78→76 and
138→168. Tonic controls also vary substantially. All endpoint/control results,
user/system components, histograms and higher costs remain in the raw record.
This shared-host comparison does not establish broad performance superiority.

## Decision and verification

Defer adoption. Any deferred wake-forwarding design needs its own race model,
correctness controls and physical CPU comparison. C13 qualification belongs to
[its recorded source](../grpc-ownership-20261024/README.md), not this candidate.

[Results and source pins](results.json) · [Manifest](manifest.json) ·
[Raw source, commands, controls and captures](raw.tar.gz)

Run `python3 check.py` to verify member/publication hashes without extraction.
ELF bytes are omitted; recorded build and executable hashes remain. These results
do not certify production readiness.

The raw archive includes a verified Git bundle of the unadopted commit. With the
baseline commit already fetched, rebuild the experiment in a detached worktree:

```sh
tar -xzf raw.tar.gz qualification/architecture-c14-source.bundle
git fetch qualification/architecture-c14-source.bundle HEAD
git worktree add --detach ../ready-set-experiment daa7eb4074d654849d4206473250933666994adb
```
