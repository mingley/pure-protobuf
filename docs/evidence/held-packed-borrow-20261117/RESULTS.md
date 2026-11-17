# Packed borrowing: full component results

Both candidates are held. Lower CPU ratios are better. Ratios compare the sum
of two thread-CPU samples for each variant within a rotated, reversed block.
“Slower blocks” counts ratios above 1; it is an observation, not a significance test.

`fx32` is packed fixed32, `fx64` is packed fixed64, `f64` is packed double,
`varint` is packed uint32, and `custom` uses a padded element with a safe codec.
`cold` constructs, encodes and drops a field; `warm` repeatedly requests its
bytes. A `copy` suffix also copies into an existing output buffer. `wire`
starts from validated wire, `clone` clones before encoding, and `mutate`
clones and appends one value before encoding.

## Original candidate: 26 cases

A is baseline, B is the original candidate, and C is an independent baseline
build. [All timing samples, allocation pairs and controls](original-results.json)
remain available.

| Case | B/A CPU median (range) | Slower blocks | Allocation requests A → B |
| --- | ---: | ---: | ---: |
| `fx32.3.cold` | 0.6522 (0.6133–0.6877) | 0/8 | 4 → 3 |
| `fx32.3.warm` | 1.0820 (1.0733–1.1113) | 8/8 | 0 → 0 |
| `fx32.3.coldcopy` | 0.6493 (0.6048–0.7256) | 0/8 | 4 → 3 |
| `fx32.3.warmcopy` | 0.9349 (0.9305–0.9472) | 0/8 | 0 → 0 |
| `fx32.64.cold` | 0.6373 (0.6139–0.6740) | 0/8 | 4 → 3 |
| `fx32.64.warm` | 1.0768 (1.0683–1.1001) | 8/8 | 0 → 0 |
| `fx32.1024.cold` | 0.6560 (0.6333–0.6806) | 0/8 | 4 → 3 |
| `fx32.1024.warm` | 1.0812 (1.0738–1.0980) | 8/8 | 0 → 0 |
| `fx32.1024.coldcopy` | 0.7696 (0.6777–0.7858) | 0/8 | 4 → 3 |
| `fx32.1024.warmcopy` | 0.7104 (0.7082–0.8004) | 0/8 | 0 → 0 |
| `fx32.16384.coldcopy` | 0.6741 (0.6705–0.6772) | 0/8 | 4 → 3 |
| `fx32.16384.warmcopy` | 0.9387 (0.9269–0.9485) | 0/8 | 0 → 0 |
| `fx64.64.cold` | 0.6753 (0.6198–0.6974) | 0/8 | 4 → 3 |
| `fx64.64.warm` | 1.0156 (1.0015–1.0276) | 8/8 | 0 → 0 |
| `f64.64.cold` | 0.6445 (0.6313–0.6647) | 0/8 | 4 → 3 |
| `f64.64.warm` | 0.8841 (0.8805–0.8919) | 0/8 | 0 → 0 |
| `fx32.64.wire` | 0.8956 (0.8889–0.9027) | 0/8 | 0 → 0 |
| `fx32.64.clone` | 0.6436 (0.6085–0.6912) | 0/8 | 3 → 2 |
| `fx32.64.mutate` | 0.7256 (0.7143–0.7345) | 0/8 | 3 → 2 |
| `fx32.1.empty` | 0.7528 (0.7359–0.7595) | 0/8 | 0 → 0 |
| `varint.3.cold` | 1.0051 (0.9770–1.0227) | 5/8 | 4 → 4 |
| `varint.3.warm` | 1.0037 (0.9642–1.0258) | 5/8 | 0 → 0 |
| `varint.1024.cold` | 1.0000 (0.9910–1.0089) | 4/8 | 4 → 4 |
| `varint.1024.warm` | 1.0071 (0.9960–1.0253) | 7/8 | 0 → 0 |
| `custom.3.cold` | 0.9904 (0.9710–1.0513) | 3/8 | 4 → 4 |
| `custom.3.warm` | 0.9984 (0.9869–1.0043) | 3/8 | 0 → 0 |

All three differential allocation pairs agreed in every case. Counts exclude
fixture preparation and output-buffer allocation. Reallocation requests are
reported separately in the raw file and did not change between variants.
Requested bytes include complete allocation and reallocation layouts; they
are not live memory, peak memory or RSS.

Examples: three fresh fixed32 values request 152 → 140 bytes; 64 values request
640 → 384 bytes; a fresh 64 KiB run with a copy requests 131,200 → 65,664 bytes.
The warmed fixed32 getter regresses in all eight blocks at all three tested
sizes. Its independent-baseline C/A medians are 0.9994, 0.9999 and 0.9997.
The original candidate is held despite its cold-path savings.

## Refined candidate: 16 cases

A is the same baseline source, B is the original candidate, and C is the
refinement. This is a separate screen; ratios from the two screens are not
pooled. [All samples and controls](refined-results.json) remain available.

| Case | Original B/A | Refined C/A | C/A slower blocks | Refined/original C/B |
| --- | ---: | ---: | ---: | ---: |
| `fx32.3.cold` | 0.6421 | 0.6481 | 0/6 | 1.0248 |
| `fx32.3.warm` | 1.0735 | 1.0411 | 6/6 | 0.9694 |
| `fx32.3.warmcopy` | 0.9358 | 0.9827 | 0/6 | 1.0547 |
| `fx32.64.warm` | 1.0872 | 1.0439 | 6/6 | 0.9609 |
| `fx32.1024.coldcopy` | 0.7763 | 0.7765 | 0/6 | 1.0034 |
| `fx32.1024.warm` | 1.0696 | 1.0386 | 6/6 | 0.9645 |
| `fx32.1024.warmcopy` | 0.7101 | 0.7102 | 0/6 | 1.0016 |
| `fx32.16384.coldcopy` | 0.6729 | 0.6734 | 0/6 | 1.0023 |
| `fx64.64.warm` | 1.0107 | 1.0635 | 6/6 | 1.0541 |
| `f64.64.warm` | 0.8892 | 0.9364 | 0/6 | 1.0537 |
| `fx32.64.wire` | 0.8872 | 1.1081 | 6/6 | 1.2450 |
| `fx32.1.empty` | 0.7438 | 0.7982 | 0/6 | 1.0728 |
| `varint.3.cold` | 1.0207 | 1.0119 | 5/6 | 0.9915 |
| `varint.3.warm` | 1.0014 | 0.9619 | 0/6 | 0.9587 |
| `custom.3.cold` | 0.9865 | 0.9938 | 3/6 | 1.0013 |
| `custom.3.warm` | 1.0008 | 0.9845 | 0/6 | 0.9786 |

Refined warmed fixed32 getter C/A ranges are 1.0340–1.0649 for three values,
1.0325–1.0533 for 64 values, and 1.0234–1.0597 for 1,024 values. Their A/A
median controls are 1.0010, 0.9988 and 0.9991. The fixed64 getter range is
1.0539–1.0775; the wire-backed fixed32 range is 1.0958–1.1287. Each of these
five cases is slower in all six blocks.

The refinement reduces the original fixed32 getter cost but leaves a repeated
regression. It also worsens fixed64 and wire-backed getter costs relative to
the original. Cold allocation savings remain. The refinement is held; no
cause is established from these timings.

[Method, decisions and validation](README.md) · [Reproduction](REPRODUCE.md)
