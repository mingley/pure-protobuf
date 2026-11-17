# Preserve backing for mixed owned-message parents

The earlier scalar-map optimization was narrowed before publication. A scalar
map value does not need retained wire itself, but creating the parent backing
can let later ordinary eager children share one buffer. Skipping that backing
for a mixed parent made 32 borrowed Blob children allocate 32 buffers instead
of sharing one.

The fix permits elision only when the enclosing schema contains singular
primitive fields and scalar-only-message maps. It rejects ordinary message,
group, repeated, payload-bearing and other-map siblings, and MessageSet parents.
The enclosing eligibility check is cached once and evaluated lazily for owned
scalar-map candidates. Default/lazy generator branches skip the new scans.

The controlled comparison uses one runtime and identical fixtures with three
generators: original unconditional ensure, the previous111 predicate, and the
narrowed fix. Every case verifies all 32 child payloads and the scalar map value.
These are allocation-event checks, not new timing measurements.

| Payload bytes | Wire order | Original borrowed | Previous111 borrowed | Fixed borrowed | Shared, all three |
| ---: | --- | ---: | ---: | ---: | ---: |
| 32 | map first | 6 | 37 | 6 | 37 |
| 32 | map last | 38 | 37 | 38 | 37 |
| 8192 | map first | 6 | 37 | 6 | 5 |
| 8192 | map last | 38 | 37 | 38 | 5 |

The previous predicate adds 31 backing allocations when the map comes first.
The narrowed fix restores the original policy in both orders, including its
one extra root allocation when the map comes last. It does not claim the
minimum possible allocation count for mixed parents.

The gate proves pure owned generated bodies are byte-identical to previous111
outside the intended mixed-parent helper. The fixed mixed helper matches the
original ensure helper exactly. Whole default/lazy outputs, owned proto2 proof
output, and owned codec-case output also match. The pure scalar-map allocation
assertion remains two events. Existing required, unknown-field, malformed-input
and sharing proofs pass, as do strict core Clippy, generator diagnostics, and
the owned consumer on Rust 1.85.

- [allocation-controls.json](allocation-controls.json) and the three raw report
  files retain every case. [previous-negative.stderr](previous-negative.stderr)
  records the old predicate failing the six-event expectation with 37 events.
- [generated-identity.json](generated-identity.json) retains the compared output
  hashes. [driver.yml](driver.yml) records exact builds, extraction, comparisons,
  commands and exit-status checks.
- [source-pins.json](source-pins.json) and the source patches bind all variants,
  fixture code, toolchains and artifact digest. No runtime Map code changed.
- [check.py](check.py) verifies hashes, raw reports, case coverage, deltas,
  output-identity coverage and proof logs. Run python3 check.py --negative-controls;
  it starts no generator or measurement.

The [earlier timing record](../owned-scalar-map-20261117/README.md) remains
historical evidence for its measured pure fixtures. Its raw data and broad
candidate patch are unchanged; this follow-up documents the release boundary.
