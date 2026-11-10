# Codegen reservation experiment, November 10

[Benchmarks](../benchmarks.md)

A per-target capacity estimate was **rejected**. In a direct generator screen it
reduced CPU in two many-file canonical-name cases, but the balanced and skewed
shared cases and the suffix-alias cases were slower in all six blocks. The screen compared baseline
[0685d7f6](https://github.com/mingley/pure-protobuf/commit/0685d7f6cbc4e0ac9cdfb020bd085a8b7eb5b2be);
the [candidate](https://github.com/mingley/pure-protobuf/commit/79cddac2b759487134924dda28d800d7203a698d)
was not merged.

The candidate changed only `src/codegen/descriptors.rs`: for multi-file,
reflection-free, shared-pool or shared-descriptor-set modes, it estimated output
reservation from a sorted index of descriptor filenames and sizes, with matcher
fallback for aliases. Default and single-file estimates kept the old reservation. This
is a capacity hint, and may change allocation behavior even when generated
text is identical.

## Generation comparison with Prost

A separate [baseline run](https://github.com/mingley/pure-protobuf/actions/runs/42765923353)
generated a seeded 1,000-message, 20-file corpus five times per profile using
pbrs at `0685d7f6` and Prost 0.14.4. The table gives independent medians;
RSS is the maximum of GNU time's direct-command peak and the periodically
sampled process-tree RSS, so sampling can miss a transient maximum.

| pbrs profile | Peak RSS MiB, pbrs / Prost | Wall ms, pbrs / Prost | Generated Rust bytes, pbrs / Prost |
|---|---:|---:|---:|
| default | 76.04 / 34.59 | 233.44 / 402.11 | 34,347,839 / 412,622 |
| lean | 46.92 / 34.54 | 179.28 / 407.25 | 12,925,901 / 412,622 |
| shared | 56.64 / 34.65 | 223.08 / 405.49 | 21,943,885 / 412,622 |

The lean pbrs profile disabled reflection, JSON and text and used the minimal
runtime; the shared-set profile was also non-default. Prost and pbrs generate
different APIs and features. This single-package corpus produces one package
file with Prost, while pbrs emits files per input plus its module registry and,
in the shared-set profile, a shared-descriptor helper. The pbrs driver disables source-location information; Prost 0.14.4 enables it by
default. Prost also uses its default syn/prettyplease formatting pass, while
pbrs writes emitted text directly. The descriptor inputs and work differ; this
run does not isolate either effect. These debug driver builds on a shared AMD EPYC 7763 host do not establish a
feature-equivalent ranking. Timings include
driver startup, protoc 3.21.12 descriptor compilation, Rust generation and
output-file writes; compiling the generated Rust is excluded.

## Reservation candidate

The [candidate screen](https://github.com/mingley/pure-protobuf/actions/runs/42767088065)
used release builds with Rust 1.99.1 and a single pinned logical CPU on a
shared AMD EPYC 9V74 host. It repeatedly passed a prepared
`CodeGeneratorRequest` to the generator; setup and warmup were outside the
process-CPU sample. For each workload, six blocks alternated baseline-first
and candidate-first order. Lower candidate/baseline ratios are better.
Same-binary controls are second/first measurements within each block.

| Workload | Candidate / baseline CPU: median (range) | Slower blocks | Baseline control range | Candidate control range |
|---|---:|---:|---:|---:|
| 20 balanced, default | 1.0030 (0.9895–1.0309) | 3/6 | 0.9821–1.0223 | 0.9903–1.0105 |
| 20 balanced, lean | 1.0189 (1.0077–1.0353) | 6/6 | 0.9543–1.0175 | 0.9714–1.0538 |
| 20 balanced, shared set | 1.0619 (1.0405–1.0688) | 6/6 | 0.9669–1.0003 | 0.9994–1.0210 |
| 20 balanced, shared pool | 1.0759 (1.0517–1.1079) | 6/6 | 0.9950–1.0157 | 0.9867–1.0091 |
| 20 skewed, lean | 1.0155 (1.0046–1.0446) | 6/6 | 0.9520–1.0164 | 0.9884–1.0909 |
| 20 skewed, shared set | 1.0260 (1.0096–1.0477) | 6/6 | 0.9920–1.0116 | 0.9695–1.0288 |
| 1 file, default | 1.0198 (1.0165–1.0295) | 6/6 | 0.9981–1.0113 | 0.9777–1.0139 |
| 1 file, lean | 1.0154 (1.0044–1.3323) | 6/6 | 0.9871–1.0538 | 0.9878–1.0170 |
| 128 canonical, lean | 0.9758 (0.9663–0.9837) | 0/6 | 0.9913–1.0060 | 0.9865–1.0058 |
| 512 canonical, lean | 0.9771 (0.9723–0.9867) | 0/6 | 0.9851–1.0098 | 0.9967–1.0081 |
| 128 aliases, lean | 1.0612 (1.0511–1.0755) | 6/6 | 0.9720–1.0234 | 0.9900–1.0046 |
| 512 aliases, lean | 1.0522 (1.0277–1.0664) | 6/6 | 0.9161–1.0055 | 0.9975–1.0038 |
| Name collision, default | 1.0015 (0.9840–1.0384) | 3/6 | 0.9625–1.0382 | 0.9638–1.0531 |
| Name collision, lean | 0.9923 (0.9748–1.0379) | 2/6 | 0.9734–1.0059 | 0.9818–1.0419 |
| Name collision, shared set | 1.0052 (0.9847–1.0283) | 5/6 | 0.9820–1.0477 | 0.9920–1.0237 |

Balanced cases use 1,000 messages spread over 20 root-level files. Skewed
cases use one large and 19 small files. Canonical cases use 128 or 512 distinct
root-level target names; alias cases use nested descriptor paths and request
their unique basenames. Collision cases include both `common.proto` and
`nested/common.proto`. Lean means `no_reflect=true,emit_json=false,emit_text=false`;
shared set and shared pool are separate opt-in settings. Sample ranges and
same-binary controls show the uncertainty of this shared-host screen; these
results do not establish a general performance ranking.

In all 15 workloads, the per-output paths, lengths and SHA-256 digests matched
between baseline and candidate during collection. The stored artifact retains a
digest of each complete file manifest, rather than the generated files or
individual manifest entries, so it cannot independently repeat that output
comparison. This is evidence for those workloads, not a complete semantic
equivalence test.

For the 20-file shared-set case, the separate output-dump process reported
45,956 KiB peak RSS at baseline and 47,816 KiB for the candidate; six timing
processes ranged 46,028–46,140 and 47,788–47,884 KiB respectively. These
are whole-process maxima from `/usr/bin/time`, not peak allocator usage.
The summed capacities of the returned output strings were equal at 22,596,435
bytes in this case. The emitter clones ordinary root-file outputs; nested
targets with a unique flat compatibility alias can instead retain the original
allocation.

## Nested-file follow-up

A [follow-up screen](https://github.com/mingley/pure-protobuf/actions/runs/42768049194)
tested the same source on a shared AMD EPYC 7763 runner, with one and two
pinned CPUs and six blocks per case. In the two-CPU case the runner's exposed
topology reported distinct cores; physical host isolation was not established.
The six table cases request nested targets with unique stems, so each emits a
nested file and a flat
compatibility alias. The table reports the sum of returned string capacities
and candidate/baseline process CPU.

| Workload | Summed capacities in bytes, baseline / candidate | One core: median (range) | Two cores: median (range) |
|---|---:|---:|---:|
| 5 files, lean | 2,719,606 / 3,465,882 | 1.0018 (0.9965–1.0251) | 0.9730 (0.9606–0.9797) |
| 5 files, shared set | 5,097,376 / 4,712,936 | 1.0095 (0.9780–1.0892) | 1.0343 (1.0237–1.0521) |
| 20 files, lean | 39,111,481 / 34,722,241 | 0.9849 (0.9781–0.9958) | 0.9927 (0.9841–1.0129) |
| 20 files, shared set | 48,782,923 / 48,492,243 | 0.9962 (0.9830–1.0120) | 1.0109 (1.0001–1.0175) |
| 128 tiny files, lean | 14,299,136 / 2,728,704 | 0.9022 (0.8988–0.9126) | 0.8920 (0.8834–0.9139) |
| 512 tiny files, lean | 208,191,488 / 10,914,816 | 0.9542 (0.9520–0.9650) | 0.9182 (0.9025–0.9256) |

The 5- and 20-file corpora contain 100 and 1,000 messages respectively; the
128- and 512-file cases use tiny two-field messages and are synthetic stress
cases. String capacity is reserved logical capacity, not resident memory.
For 20-file lean, one-core whole-process peak RSS across timing samples was
50,152–50,184 KiB at baseline and 50,104–50,188 KiB with the candidate. The
five-file lean capacity instead grew. With two cores, both shared-set cases
were slower in all six blocks.

For the six table cases, same-binary control ratios ranged 0.9685–1.0242
(baseline) and 0.9497–1.0281 (candidate) on one core, and 0.9415–1.0767 and
0.9791–1.0302 on two cores. Their ranges and this single shared host limit the
conclusions. Separate requested-live allocator instrumentation excludes
allocator overhead and physical realloc peaks; its atomic bookkeeping can
perturb execution. Timing binaries did not use that instrumentation.

The [nested data](codegen-reservation-20261110-nested.json) retains all 34
case/core combinations, 204 timing pairs, controls and 68 allocation samples.
Per-file paths, lengths and SHA-256 digests matched during collection; only a
digest of each complete manifest is retained. The
[verification run](https://github.com/mingley/pure-protobuf/actions/runs/42769437122)
recomputed the ratios and checked both output summaries and the allocation
measurements. Its [90-day bundle](https://github.com/mingley/pure-protobuf/actions/runs/42769437122/artifacts/13741187192)
has SHA-256
`d7e0214135d097b4b897b9705dcef53840b887830dae3c1fef16423dc4d5a4e1`
and expires February 8, 2027. While available, verify the nested data from the
repository root:

```sh
(
  set -eu
  repo_dir="$PWD"
  temp_dir="$(mktemp -d)"
  trap 'rm -rf "$temp_dir"' EXIT
  cd "$temp_dir"
  gh run download 42769437122 --repo mingley/pure-protobuf \
    --name verified-codegen-reserve-nested-20261110 --dir bundle
  printf '%s  %s\n' \
    2a04aa2a25bc6e693cb08ac885cf29532a1d74d0dfa7e70e40caad6ae2f8ad73 \
    bundle/nested.zip | sha256sum --check
  mkdir -p evidence/source
  unzip -q bundle/nested.zip -d evidence/source
  python3 "$repo_dir/docs/benchmarks/codegen-reservation-20261110-nested-verify.py"
  cmp evidence/codegen-reserve-nested-20261110.json \
    "$repo_dir/docs/benchmarks/codegen-reservation-20261110-nested.json"
)
```

## Data and verification

The [retained data](codegen-reservation-20261110.json) includes all 90 paired
timings, same-binary controls, 30 separate allocation samples, output-manifest
digests and the three baseline summary matrices. The allocation counter records
requested bytes and calls; it does not measure live or peak memory.
[Verification](https://github.com/mingley/pure-protobuf/actions/runs/42768647583)
checked source and fixture hashes, the original artifact digests, the recorded
Prost matrices, candidate-screen ratios and summaries, and scratch-ref
cleanup. The [90-day bundle](https://github.com/mingley/pure-protobuf/actions/runs/42768647583/artifacts/13740952160)
has SHA-256
`639b47a5c4cedc4eaa41cb2661657ea8e6252c08b9ebbe8dfe38d8ffc78d0bb4`
and expires February 8, 2027. It contains the original artifacts and fixture
sources. To repeat the data check while it is available:

```sh
(
  set -eu
  repo_dir="$PWD"
  temp_dir="$(mktemp -d)"
  trap 'rm -rf "$temp_dir"' EXIT
  cd "$temp_dir"
  gh run download 42768647583 --repo mingley/pure-protobuf \
    --name verified-codegen-reserve-20261110-v2 --dir bundle
  printf '%s  %s\n' \
    69bcb203fc49f2e4eea2493ca929bc8a668d6cffef4aec0515f503baa7097a09 bundle/baseline.zip \
    8ee951f6bade6d23ce86636fd1561a7a5124b9d986f592ea04d46bf99aa1eed4 bundle/candidate.zip \
    | sha256sum --check
  mkdir -p evidence/baseline evidence/candidate
  unzip -q bundle/baseline.zip -d evidence/baseline
  unzip -q bundle/candidate.zip -d evidence/candidate
  python3 "$repo_dir/docs/benchmarks/codegen-reservation-20261110-verify.py"
  cmp evidence/codegen-reserve-20261110.json \
    "$repo_dir/docs/benchmarks/codegen-reservation-20261110.json"
)
```

