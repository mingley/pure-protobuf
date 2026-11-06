# Fresh encoding warmup, November 6

The development-loop benchmark warmed the same parsed objects it later
measured as fresh. Warmup could populate pbrs' packed-field caches, so that
row mixed first and subsequent encodes. Warmup now uses a separate set of
parsed messages for pbrs, Prost and upstream v4. Each measured object is
encoded once; preparation remains outside the measured window.

This debug allocation diagnostic uses source `e72629e4` and an overlay changing
only `bench/devloop/src/main.rs`. Each process measures four encodes from eight
prepared objects. Twelve processes per variant cover all three codecs and
warmup counts of zero, one, four and eight. The TAT references use locked
Prost 0.13.5 and upstream protobuf `4.35.1-release`; the framework's separate
Prost 0.14.4 consumer suite is outside this diagnostic.

| Codec | Old calls at warmup 0 / 1 / 4 / 8 | New calls at every warmup count |
|---|---:|---:|
| pbrs | 28 / 22 / 4 / 4 | 28 |
| Prost | 20 / 20 / 20 / 20 | 20 |
| Upstream v4 | 4 / 4 / 4 / 4 | 4 |

These are totals over four encodes. The corrected pbrs row requests 796 bytes,
versus 992 for Prost and 348 for v4. With all four pbrs objects warmed, the old
row requested only 348 bytes. The correction exposes a larger cold-encoding
allocation cost; it is a measurement repair, not a runtime speedup. The capture
retains wall times, but they are not compared or used for a performance claim.

New child and aggregate reports identify this lifecycle with
`fresh_encode_profile: "independent-warmup-v1"`. The collector requires it for
fresh rows. Comparison rejects mixing corrected and historical lifecycles;
older reports remain readable. Historical results keep their original numbers
and this limitation. Rerun both revisions with the same lifecycle before
drawing a fresh-encoding conclusion.

The candidate passes 25 benchmark unit tests, five comparison integration
tests and strict Clippy. Source and build receipts retain 1,344 selected input
hashes for each variant. Both builds use the same source path and ambient
stable Rust toolchain. The archive includes exact source versions, the source
patch and all 24 child outputs; binaries and caches are omitted. The capture
controller was an inline command and is not retained by value.

Run `python3 docs/evidence/fresh-encode-warmup-20261106/check.py` in this checkout.
It reconstructs selected inputs, verifies retained receipts and recomputes
allocation totals. Peer source trees and toolchain internals are outside the
selected inventory. It does not replay binaries, attest the compiler, cover
all compiler reads or establish production qualification.

[Manifest](manifest.json) · [Sources and observations](raw.tar.gz)
