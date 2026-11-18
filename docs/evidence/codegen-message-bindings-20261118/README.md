# Field bindings during map decoder emission

The [candidate](https://github.com/mingley/pure-protobuf/commit/8fdf4f106daa67ddb7a4187fcf7d1b430e548b9d) removes the second binding pass for each emitted message. The caller emits the message and then its map decoders using the same descriptor and worker. Message emission binds field names, boxed fields, tree-backed maps and shared bytes before rendering. The codegen-only patch reuses that binding in the following emitter. [Baseline](https://github.com/mingley/pure-protobuf/commit/9061b8ee0a75d32acb19ebf7090e4a95660012db) · [Replay patch](candidate.patch)

## Generator measurements

The shared Linux runner uses one or two pinned guest-distinct cores. Each measured cell runs eight alternating ABBA blocks with paired Rust 1.99.1 binaries, thin LTO and one codegen unit. The CPU interval measures repeated prepared requests after warmup, including output destruction. Protoc, file writes and consumer compilation occur outside it. Separate allocation binaries count successful allocation and reallocation requests.

| Corpus | Allocation requests, one core | Candidate / baseline CPU, one core | Candidate / baseline CPU, two cores |
| --- | ---: | ---: | ---: |
| 6 messages / 2 files, default | 1,853 → 1,704 | 0.9631 (0.9282–1.0149) | 0.9660 (0.9188–0.9813) |
| 1,000 messages / 20 files, lean | 169,030 → 144,935 | 0.9603 (0.9499–0.9867) | 0.9218 (0.9071–0.9347) |
| OTLP, default | 7,848 → 6,843 | 0.9643 (0.9553–0.9719) | 0.9606 (0.9101–0.9735) |
| Map and storage selectors | 958 → 825 | 0.9284 (0.9065–0.9330) | 0.9506 (0.9355–0.9663) |

The 1,000-message lean cells save 24,095 allocation requests and 1,237,064 cumulative requested bytes per generation. The one-core small CPU range spans 1.0. [Raw records](results.json) retain all 64 blocks, 256 timed process samples, 48 allocation samples and within-binary controls. Each comparison uses the same baseline as the [field-name experiment](../field-names-20261118/README.md) and describes this individual patch.

## Generated output and selectors

The [screen](https://github.com/mingley/pure-protobuf/actions/runs/44349216101) compared file names and exact bytes in six cases, in original and reversed request orders at both core settings. The measured cases include seeded small and 1,000-message inputs and the five pinned OTLP sources. The two additional map cases exercise normal emission and `owned_message_fields=true` with map ordering, boxed message and shared byte selectors. The selected-map consumer round-tripped both colliding map accessor names, a boxed child and selected bytes, including an accessor with an occupied name.

The gate verified the only map-decoder call follows the binding message emitter. Rust 1.85 passed 42 codegen tests and the codegen-only build. Stable passed the same generator tests, formatting, all-feature library Clippy with denied warnings, and the sequential and parallel mixed-config isolation tests.

## Verify the retained capture

The [raw archive](https://github.com/mingley/pure-protobuf/actions/runs/44349216101/artifacts/14447182993) retains both pinned source records, the probe manifest and lockfile, request bytes, output manifests and selected consumer source until 2027-02-16. The [independent verification](https://github.com/mingley/pure-protobuf/actions/runs/44350181053) rechecks the archive, output manifests, consumer file bytes, selector parameters and arithmetic. The [screen workflow](screen.yml), [corpus builder](corpus.py) and [comparison driver](compare.py) record the exact capture commands.

Apply [candidate.patch](candidate.patch) to the pinned baseline to reproduce `src/codegen/parse.rs` Git blob `2481cfb29c69a956c657fb00a8b67b2c6e9632c9`.

From the repository root, while the archive is available, run the same verification.

```sh
work="$(mktemp -d)"
gh api repos/mingley/pure-protobuf/actions/artifacts/14447182993/zip > "$work/original.zip"
printf '1662c301aa2b19260882f56a26f391d797f61baa4b72e54de50897098a8def01  %s\n' "$work/original.zip" | sha256sum --check
unzip -q "$work/original.zip" -d "$work/original"
python3 -B docs/evidence/codegen-message-bindings-20261118/verify.py \
  "$work/original" "$work/results.json"
python3 - "$work/results.json" docs/evidence/codegen-message-bindings-20261118/results.json <<'PY'
import json
from pathlib import Path
import sys

actual, expected = [json.loads(Path(path).read_text()) for path in sys.argv[1:]]
assert actual == expected
PY
```
