# Compatibility checks and encode scratch

The hosted run at `e7595eb7` exposed three compatibility problems: a generic
`WireOut` helper prevented dynamic dispatch, native Rust 1.85 tests pulled in a
Rust 1.88-only test peer, and shared-consumer checks used a stale suite inventory.
This record preserves the failures, repairs and source-specific checks. A local
repair is separate from a completed hosted CI run.

## Compatibility repairs

| Problem | Change | Evidence |
|---|---|---|
| `WireOut` lost dynamic compatibility | Restrict only the new generic contiguous helper to `Self: Sized`; keep its body unchanged | Public consumer fails with E0038 before the fix; 113 core and 2 public-consumer checks pass after it |
| Shared consumers pass but CI rejects them | Classify pinned upstream, local and excluded suites in one inventory; check executed identities | The original hosted execution passes 20 suites / 248 tests, then the stale 19-suite check fails |
| Rust 1.85 native tests select a Rust 1.88 peer | Keep the optional tonic gzip oracle out of the default native dependency graph | Repaired Rust 1.85 library build and 62 memory checks pass; Rust 1.88 adapter checks pass 33 plain / 34 gzip |

The shared suite inventory retains the 19 upstream suites, one local arena
regression suite and five documented upstream exclusions. Missing, unexpected,
duplicate, empty or failing suites must still fail the gate. Nested and explicit
Cargo test targets are included in the identity checks. The original local
arena test is not an upstream file and is not silently discarded.

Fourteen Python contract checks pass. Review found that the first version missed
nested Cargo test targets; three detecting controls fail against that version and
pass after the repair. Replaying the original 20-suite / 248-test log checks the
new report parser without claiming a new Rust test run.

The original public API run passes 195 checks and fails one dynamic-compatibility
check. That regression predates this update. The helper's concrete generic calls
and specialized contiguous writes remain available. The full semver tool is not
installed locally; the corrected hosted run still needs to check all three crates.

## Encode scratch experiment

**Not adopted.** The follow-up to `06ec9f23` opts Prost into private append-only
storage. Custom codecs and the pbrs blanket keep the explicit legacy `WireOut`
sink, avoiding the earlier temporary allocation and copy on every message.

**Requested bytes at the scratch and serialization sites, four messages:**

| Workload | Earlier bridge | Opt-in fallback |
|---|---:|---:|
| Custom, 1 KiB | 5.05 KiB | 1.00 KiB |
| Custom, 1 MiB | 5.00 MiB | 4.00 MiB |
| Prost, 1 MiB, plaintext scratch only | 1.00 MiB | 1.00 MiB |

Custom serialization allocations fall from **8 to 0** for both sizes. Both actual
paired runs pass **34 cases / 88 independently decoded frames**; all 32 Prost
rows match exactly. The original-runtime Prost scratch count was 4.00 MiB, so its
75% site reduction remains intact. These are allocation-site measurements, not
whole-endpoint CPU, latency, throughput, retained memory or RSS results.

The selected library run passes **706 checks**, with one existing ignore and
**34 listener-dependent checks filtered**. The first full run records 705 passes
and 35 failures: 33 print EPERM, one source-bind case finds no usable address,
and one new detector catches a real draft defect. Passing bare `BytesMut` still
used the temporary bridge; an explicit legacy sink fixes it. Strict selected
Clippy and formatting pass. The fresh-process control balances **14 leases
acquired / 14 released**, including completed futures kept alive and codec changes.

Test-profile scratch and batch layouts grow **32→40 B / 192→200 B**. Shipping
costs are unmeasured. Large canonical idle storage has its own 4 MiB per-buffer /
16 MiB process allowance. The legacy 64 KiB policy checks visible `BytesMut`
capacity only; custom splits or replacements can retain larger backing storage.
No full allocation bound is claimed for that legacy path. Active work, messages,
wire output and the separate decode allowance remain outside the encode ledger.

## Records and reproduction

[Results](results.json) · [Raw text records](records.json) ·
[Experimental source patch](source.patch.json) · [Manifest](manifest.json)

Run `python3 check.py` to verify publication files and original record bytes
without extracting anything. Records include failed runtime/lint/preparation
attempts, commands, source hashes, counter rows and private probe helpers.
Executable bytes and repetitive no-run Cargo compiler-JSON output are omitted;
the omission inventory retains their sizes and hashes. Local absolute paths
identify the original execution environment.

The experimental patch applies to `06ec9f23`, retained in the bytes Git bundle in
the [earlier capsule](../grpc-required-merge-20261026/README.md). Restore that base,
decode the patch JSON, and use the recorded build/probe commands:

```sh
python3 - <<'PYTHON'
import json
from pathlib import Path
patch = json.loads(Path("source.patch.json").read_text())
Path("candidate.patch").write_bytes(patch["patch"].encode("utf-8"))
PYTHON
git apply candidate.patch
```

Private allocator
instrumentation is removed from the final candidate, with an exact restoration
check. The pbrs blanket remains on the legacy path; Prost explicitly opts in.

Endpoint comparisons, current-source full CI, leak checks and a 24-hour soak
remain separate gates. These records do not prove production readiness or broad
superiority over tonic/prost.
