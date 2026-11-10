# Target-file match cache experiment, November 10

[Benchmarks](../benchmarks.md)

A scratch-only one-entry cache reduced repeated target-file matching for
grouped messages. It is **held**: the same screen showed possible regressions
for interleaved and some unique-file inputs. The measurements compare two
pbrs revisions; they do not establish a result against Prost.

The baseline is `5fe70efdc798031badefd1a3847d8866dde352f5`; candidate
`540e12e1a71d6ebdad86bb55b36ab89711451455` changes only the message-selection
loop in `src/codegen/descriptors.rs`. For each target it remembers the last
original file string and four file-derived match results. A different string
uses the existing file-facts and matcher logic. Map, external-type, WKT,
dependency and emission decisions remain per type; the loop still sorts and
deduplicates its selected names. The cache remains correct for interleaved files, but only consecutive
evaluated messages from the same file reuse the entry.

The earlier [instruction diagnostic](https://github.com/mingley/pure-protobuf/actions/runs/42783012988)
observed 60,000 target-matcher calls across three grouped generations. In
separate counter-only builds of this experiment, the grouped input went from
20,000 calls per generation to 400. An intentionally interleaved input
remained at 20,000 calls for both builds. Disabling cache invalidation changed
20 output files in that interleaved case. Counters and the mutant were never
used for CPU timing.

## Correctness and method

The [screen](https://github.com/mingley/pure-protobuf/actions/runs/42785753857)
compared returned output names and bytes for successful requests, and exact
error strings for rejected requests, at one and two workers. The 18 requests
included grouped and interleaved messages, 128- and 512-file shapes, suffix
aliases, ambiguous and colliding names, external types and services,
public imports with and without generated owners, `emit_deps`, a well-known
type, a synthetic all-files request and a duplicate descriptor. The candidate
matched the baseline in every case. The one-worker outputs for grouped lean,
interleaved, service, public-import and WKT cases are retained as files; other
successful cases retain individual output hashes after the collection-time
byte comparison. These checks do not prove complete feature parity.

The candidate also passed workspace formatting and four descriptor matcher,
determinism and parallel-target tests on Rust 1.85 in
[this preflight](https://github.com/mingley/pure-protobuf/actions/runs/42785301318),
plus generated-consumer tests for cross-package stem collisions and external
type mappings in the screen. The earlier preflights failed before testing:
[one](https://github.com/mingley/pure-protobuf/actions/runs/42784768945) used
an insufficient checkout depth, and [the next](https://github.com/mingley/pure-protobuf/actions/runs/42785018858)
found a line-wrap difference; both scratch refs were removed.

Release binaries used Rust 1.99.1, thin LTO and one codegen unit on a shared
AMD EPYC 9V45 runner. Each case used six alternating-order blocks on one or
two guest-distinct pinned cores, with same-binary controls. The probe repeatedly
passed a prepared in-memory request to codegen; process CPU includes its
worker threads and output destruction, and excludes protoc, process startup,
output-file writes and generated-code compilation.

The grouped input has 1,000 messages in 20 files. The interleaved input also
has 1,000 messages in 20 files, but arranges sorted type names to alternate
between files; it uses a different field mix, so compare candidate and baseline
within each row. The many-file cases have one two-field message per file;
alias128 requests basenames for nested files. The service case has two target
files, an imported external type and generated client stubs. Lean disables
reflection, JSON and text.

## Process CPU

Entries are candidate/baseline median (range; slower blocks). Controls are
the range of the second same-binary sample divided by the first, not confidence
intervals.

| Case | Workers | Candidate / baseline | Baseline control | Candidate control |
|---|---:|---:|---:|---:|
| balanced20-default | 1 | 0.9773 (0.8903–0.9946; 0/6) | 0.9530–1.0134 | 0.9935–1.0332 |
| balanced20-default | 2 | 0.9714 (0.9376–1.0065; 1/6) | 0.9216–1.0026 | 0.9767–1.0242 |
| balanced20-lean | 1 | 0.9330 (0.8058–0.9659; 0/6) | 0.9061–1.0109 | 0.8764–1.1049 |
| balanced20-lean | 2 | 0.9409 (0.9057–1.0176; 1/6) | 0.9736–1.0341 | 0.9405–1.0265 |
| interleaved-lean | 1 | 1.0333 (0.9704–1.1165; 4/6) | 0.9344–1.0174 | 0.9865–1.0442 |
| interleaved-lean | 2 | 1.0125 (0.9924–1.0357; 5/6) | 0.9872–1.0317 | 0.9808–1.0141 |
| many128-lean | 1 | 0.9884 (0.8687–1.0078; 1/6) | 0.9704–1.2498 | 0.9605–1.0210 |
| many128-lean | 2 | 1.0163 (1.0055–1.0713; 6/6) | 0.9860–1.0237 | 0.9562–1.0132 |
| many512-lean | 1 | 1.0131 (0.9404–1.0307; 4/6) | 0.9419–1.0550 | 0.9559–1.0610 |
| many512-lean | 2 | 1.0031 (0.9806–1.0338; 3/6) | 0.9477–1.0167 | 0.9569–1.0588 |
| alias128-lean | 1 | 1.0182 (0.9579–1.0442; 5/6) | 0.9702–1.0184 | 0.9759–1.0267 |
| alias128-lean | 2 | 1.0158 (1.0021–1.0311; 6/6) | 0.9918–1.0237 | 0.9913–1.0008 |
| services-lean | 1 | 1.0010 (0.9907–1.0100; 3/6) | 0.9769–1.0259 | 0.9803–1.0304 |
| services-lean | 2 | 0.9978 (0.9789–1.0169; 3/6) | 0.9541–1.0136 | 0.9855–1.0142 |

All three allocation samples for each variant, case and worker count had the
same successful allocation/reallocation request counts and cumulative
requested bytes. The instrumented allocator ran separately from timing; these
figures are neither peak heap nor RSS. The ranges overlap the small observed
differences on several holdouts, and a single shared runner cannot establish a
general CPU win or loss. The observed grouped CPU direction and
interleaved/unique-file tradeoff warrant further work before adoption.

## Data and verification

[Extracted data](codegen-target-cache-20261110.json) retain all 84 CPU
blocks, their controls and 84 allocation samples. Run
`python3 docs/benchmarks/codegen-target-cache-20261110-verify.py` from the
repository root to recompute the summaries. The
[experiment sources](../evidence/codegen-target-cache-20261110/) preserve the
candidate patch, probe, fixtures, counter/mutant overlays and workflow.

The [independent verification](https://github.com/mingley/pure-protobuf/actions/runs/42786852338)
checked the original ZIP digest, 18 request hashes, the retained output
bytes, matching recorded per-file hashes, both counter witnesses, mutant
output, timing arithmetic and allocation samples. The
[verified bundle](https://github.com/mingley/pure-protobuf/actions/runs/42786852338/artifacts/13749628034)
expires February 8, 2027 at 08:58:45 UTC; ZIP SHA-256:
`5e8f3ca82eb883b5111ba8b5ba7b82dea202be61c94dfd67880f176ff278a5ae`.
It includes the original screen ZIP, extracted inputs and outputs, and the
archive verifier. While available, verify it from the repository root:

```sh
(
  set -eu
  repo_dir="$PWD"
  temp_dir="$(mktemp -d)"
  trap 'rm -rf "$temp_dir"' EXIT
  gh run download 42786852338 --repo mingley/pure-protobuf \
    --name verified-target-cache-screen-5fe --dir "$temp_dir"
  printf '%s  %s\n' \
    fd6a43e97b3db5e5a2fceb5c1b8da5ba1f23e0732531eacaa662c059ce4111d7 \
    "$temp_dir/target.zip" | sha256sum --check
  mkdir -p "$temp_dir/evidence/target"
  unzip -q "$temp_dir/target.zip" -d "$temp_dir/evidence/target"
  cd "$temp_dir"
  python3 "$repo_dir/docs/evidence/codegen-target-cache-20261110/verify-archive.py"
)
```
