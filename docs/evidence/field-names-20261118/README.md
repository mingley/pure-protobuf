# Field-name allocation during code generation

The [candidate](https://github.com/mingley/pure-protobuf/commit/f20b8f49d6f3889e4afbae3427eae061ad081195) shares immutable field spellings between the collision reservation sets and the per-message emitter caches. Reservations keep the same lexical ordering, traversal and suffix selection. The [baseline](https://github.com/mingley/pure-protobuf/commit/9061b8ee0a75d32acb19ebf7090e4a95660012db) uses separate string allocations for those reservations.

## Generator measurements

The probe repeatedly passes a prepared `CodeGeneratorRequest` to a Rust 1.99.1 release build with thin LTO and one codegen unit. The Linux runner pins one or two guest-distinct cores. Each cell uses eight alternating ABBA blocks. A block divides the sum of its two candidate CPU samples by the sum of its two baseline samples. The probe excludes protoc, input setup, warmup, output-file writes and generated-code compilation from the measured interval.

The separate allocation build counts successful allocation and reallocation requests. It counts the full requested size on realloc. Requested bytes and process RSS describe different quantities. The table shows one request after warmup for allocation counts and the median and full range of paired ratios for CPU.

| Corpus | Allocation requests, one core | Candidate / baseline CPU, one core | Candidate / baseline CPU, two cores |
| --- | ---: | ---: | ---: |
| 6 messages / 2 files, default | 1,853 → 1,753 | 0.9795 (0.9767–0.9873) | 0.9798 (0.9553–0.9941) |
| 1,000 messages / 20 files, default | 291,760 → 275,684 | 0.9885 (0.9753–1.0073) | 0.9849 (0.9547–1.0052) |
| 1,000 messages / 20 files, lean | 169,030 → 152,954 | 0.9759 (0.9576–0.9828) | 0.9594 (0.9340–0.9696) |
| OTLP, default | 7,848 → 7,252 | 0.9829 (0.9587–0.9864) | 0.9769 (0.9651–0.9851) |
| OTLP, lean | 7,509 → 6,913 | 0.9774 (0.9707–0.9845) | 0.9984 (0.9664–1.0247) |
| 128 small files, lean | 53,811 → 52,787 | 0.9642 (0.9591–0.9676) | 0.9718 (0.9588–0.9805) |

The 1,000-message cases save 16,076 allocation requests and 464,608 cumulative requested bytes per generation on both core settings. OTLP lean on two cores has a CPU range spanning 1.0 and a wall ratio median of 1.0073. Both large default CPU ranges also span 1.0. Peak RSS ranges overlap in each cell. [Raw records](results.json) retain all 384 timed process samples, their within-binary controls, 72 separate allocation samples, output-manifest digests and hashed inputs.

The seeded inputs use the repository's CG-19 seed and renderer. The OTLP inputs are the five hash-verified v1.7.0 sources from SB-09. Default refers to reflection, JSON and text enabled with service stubs disabled. Lean disables those three metadata and format outputs.

## Generated consumers

The [screen](https://github.com/mingley/pure-protobuf/actions/runs/44347412194) compared output paths and bytes for every corpus and a proto2 collision fixture with reserved, normalized, borrowed-view and keyword accessors. Each comparison used the ordinary and reversed target and descriptor orders at both core settings. The first consumer step selected the wrong external path after the screen had retained the correct files. The [replay](https://github.com/mingley/pure-protobuf/actions/runs/44348981830) verified the archive digest and per-file hashes, recomputed all 96 timing blocks, compiled the 1,000-message lean consumer and ran the small, OTLP and collision consumers against the retained Rust files.

Rust 1.85 passed 42 generator tests and the codegen-only build. The stable lane passed 42 generator tests, formatting, all-target/all-feature pbrs Clippy with denied warnings, and four plugin and build-script consumer checks. The optional Rust 1.85 Clippy command reported 14 style diagnostics in files outside the candidate diff. [Rust 1.85 logs](https://github.com/mingley/pure-protobuf/actions/runs/44347412194/job/134953456814) · [Stable logs](https://github.com/mingley/pure-protobuf/actions/runs/44347412194/job/134953457215)

## Verify the retained capture

The [raw archive](https://github.com/mingley/pure-protobuf/actions/runs/44347412194/artifacts/14445684438) keeps the requests, exact output manifests, selected consumer sources, lockfile and build logs until 2027-02-16. The [verification archive](https://github.com/mingley/pure-protobuf/actions/runs/44348981830/artifacts/14447382144) keeps the replay results and consumer logs until 2027-02-16. The [screen workflow](screen.yml), [corpus builder](corpus.py), [comparison driver](compare.py) and [replay workflow](replay.yml) retain the commands and source pins for each run.

Run from the repository root while the original archive is available.

```sh
work="$(mktemp -d)"
gh api repos/mingley/pure-protobuf/actions/artifacts/14445684438/zip > "$work/original.zip"
printf 'b6a5bd65fdff26b09c121beccbb7f81f51cb026038f67384e5838f6c6b31cfb7  %s\n' "$work/original.zip" | sha256sum --check
unzip -q "$work/original.zip" -d "$work/original"
python3 -B docs/evidence/field-names-20261118/verify.py \
  "$work/original" "$work/results.json"
python3 - "$work/results.json" docs/evidence/field-names-20261118/results.json <<'PY'
import json
from pathlib import Path
import sys

actual, expected = [json.loads(Path(path).read_text()) for path in sys.argv[1:]]
assert actual == expected
PY
```
