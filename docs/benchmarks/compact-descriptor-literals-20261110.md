# Compact descriptor literals, November 10

[Benchmarks](../benchmarks.md)

A held codegen experiment writes printable descriptor bytes directly inside a
Rust byte string, escaping quotes, backslashes and other bytes. The generated
public constant and decoded descriptor bytes stay the same; source spelling
changes. The candidate reduced reflective output and generation CPU on the
measured synthetic schemas, but a two-worker lean/shared-pool slowdown
repeated. The candidate is **not merged**.

The comparison used baseline
[`5fe70efd`](https://github.com/mingley/pure-protobuf/commit/5fe70efdc798031badefd1a3847d8866dde352f5)
and candidate
[`f2cca721`](https://github.com/mingley/pure-protobuf/commit/f2cca72100959a3dcce4f68ca003f7f573066726).
The [retained patch](../evidence/compact-descriptor-literals-20261110/candidate.patch)
applies to that baseline. These screens do not compare pbrs with Prost and do
not establish an overall ranking.

## Generation CPU

Values are candidate CPU divided by baseline CPU: median (range; slower
pairs). Lower is faster. The first [screen](https://github.com/mingley/pure-protobuf/actions/runs/42776117586)
used 18 pairs per case; the [repeat](https://github.com/mingley/pure-protobuf/actions/runs/42780774644)
used 24. Both used alternating order, same-binary controls, Rust 1.99.1 and
one or two pinned guest-visible cores on shared AMD EPYC 7763 runners. Their
baseline binaries matched each other by SHA-256, as did their candidate
binaries. The runners' physical independence is unknown.

| Input and profile | Workers | 18 pairs | 24 pairs |
|---|---:|---:|---:|
| single-default | 1 | 0.9906 (0.9298–1.0432; 4/18) | — |
| single-default | 2 | 0.9934 (0.9563–1.0079; 4/18) | — |
| balanced20-default | 1 | 0.9209 (0.9049–0.9404; 0/18) | — |
| balanced20-default | 2 | 0.9073 (0.8386–0.9310; 0/18) | — |
| nested5-default | 1 | 0.9619 (0.8979–0.9790; 0/18) | 0.9622 (0.9479–0.9812; 0/24) |
| nested5-default | 2 | 0.9442 (0.9360–0.9804; 0/18) | 0.9442 (0.8865–0.9815; 0/24) |
| nested5-lean | 1 | 0.9943 (0.9767–1.0166; 6/18) | 0.9922 (0.9750–1.0076; 6/24) |
| nested5-lean | 2 | 1.0227 (0.9814–1.0390; 17/18) | 1.0174 (0.9852–1.0321; 22/24) |
| nested5-shared_pool | 1 | 1.0012 (0.9788–1.0096; 11/18) | 1.0023 (0.9825–1.0444; 15/24) |
| nested5-shared_pool | 2 | 1.0101 (0.9915–1.0472; 17/18) | 1.0126 (0.9890–1.1838; 19/24) |
| balanced20-lean | 2 | 0.9952 (0.9565–1.0150; 6/18) | 0.9986 (0.9738–1.0250; 11/24) |
| balanced20-shared_pool | 2 | 0.9908 (0.9687–1.0063; 3/18) | 0.9966 (0.9691–1.0199; 10/24) |

`single` has 50 messages and one target; `nested5` has 100 messages and
five targets; `balanced20` has 1,000 messages and 20 targets. `default`
embeds descriptors, `lean` disables reflection, JSON and text, and
`shared_pool` uses an external shared pool. Each process receives a prepared
`CodeGeneratorRequest`; process CPU excludes request setup and warmup, and
does not include compiling the generated Rust.

On two workers, `nested5-lean` was slower in 17/18 and 22/24 pairs;
`nested5-shared_pool` was slower in 17/18 and 19/24. Their output files were
byte-identical across variants in the timing runs' in-memory comparisons;
those output trees were not retained in the timing artifacts. A separately
instrumented [witness](https://github.com/mingley/pure-protobuf/actions/runs/42782528800)
confirmed that those two paths skipped the descriptor renderer while the
default path entered it. The same-binary control ranges overlap the observed
differences. For lean, baseline/candidate controls were 0.9565–1.0285 /
0.9917–1.1518 in the first screen and 0.9763–1.0198 / 0.9439–1.0252 in the
repeat. For shared-pool they were 0.9792–1.0525 / 0.9837–1.0353 and
0.9569–1.0232 / 0.9592–1.0452. An [inspection of the exact measured binaries](https://github.com/mingley/pure-protobuf/actions/runs/42787250299)
found that several selected generator functions moved by 144 bytes in the
candidate. This does not establish the cause of the timing difference; code
layout and shared-host conditions remain possible factors.

For the 1,000-message default case, generated Rust fell from 34,347,839 to
27,556,279 bytes. One-worker whole-process peak RSS samples were
60,996–61,124 versus 53,372–53,504 KiB. A separate allocator-instrumented
two-operation sample reported 581,378 requests in both variants and
247,211,496 versus 233,628,376 requested bytes. Requested bytes are not peak
live memory or total process RSS.

## Correctness and remaining work

The [Rust 1.85 and stable checks](https://github.com/mingley/pure-protobuf/actions/runs/42773191705)
covered all 256 byte values, escaping, adjacent hex digits, descriptor
round-trip, shared descriptors, compatibility, Clippy and rustdoc.
[Conformance and pinned regeneration](https://github.com/mingley/pure-protobuf/actions/runs/42773937503)
also passed. A [committed-input snapshot](https://github.com/mingley/pure-protobuf/actions/runs/42774602186)
compared all 45 generated files: seven changed only in descriptor-literal
spelling, with decoded bytes and all surrounding source equal. The lean and
shared-pool output was unchanged.

## Compiling generated code

The [consumer comparison](https://github.com/mingley/pure-protobuf/actions/runs/42776476520)
compiled the retained generated files against the same baseline runtime. It
used dependency-warm `cargo check` and release builds, plus incremental checks
with separate warmed caches and a changing fixed-width source marker. Clean
builds removed only consumer artifacts. Each sample used one pinned logical
CPU on a shared AMD EPYC 7763 runner. Source copying and executable validation
were outside the measured CPU time.

| Input and profile | Build | Candidate / baseline CPU: median (range; slower pairs) |
|---|---|---:|
| single-default | check | 0.9961 (0.9749–0.9968; 0/4) |
| nested5-default | check | 0.9963 (0.9801–1.0014; 1/4) |
| balanced20-default | check | 1.0027 (0.9943–1.0353; 2/3) |
| balanced20-lean | check | 1.0028 (0.9937–1.0175; 2/3) |
| balanced20-shared | check | 0.9968 (0.9905–1.0067; 1/3) |
| single-default | release | 1.0079 (0.9996–1.0184; 3/4) |
| nested5-default | release | 0.9894 (0.9894–1.0071; 1/3) |
| nested5-default | incremental | 0.9908 (0.9797–0.9966; 0/3) |
| balanced20-default | incremental | 1.0030 (0.9921–1.0263; 2/3) |

The `shared` profile uses opt-in shared descriptor storage. The 20-target
lean files were byte-identical between variants. With only three or four pairs
per case and overlapping same-binary controls, these results do not establish a
consumer-compilation speedup or rule out a small cost. For 20-target default
clean checks, baseline/candidate control ranges were 0.9879–1.0215 /
0.9820–1.0281; for incremental checks they were 0.9919–1.0094 /
0.9604–1.0278.

For the 20-target default clean checks, maximum child RSS samples were
4,333,480–4,333,924 KiB baseline and 4,326,480–4,327,008 KiB candidate;
this is not a process-tree peak. The single-target release executables were
both 928,104 bytes with the same SHA-256, and the five-target release
executables were both 1,561,224 bytes with the same SHA-256. Those binary
observations apply to these fixtures. Rust compiler allocation counts were not
measured.


## Check the retained data

The [18-pair data](compact-descriptor-literals-holdout-20261110.json) and
[24-pair data](compact-descriptor-literals-repeat-20261110.json) retain every
pair, same-binary control, allocation sample and output summary. The
[independent holdout audit](https://github.com/mingley/pure-protobuf/actions/runs/42777695391)
recomputed 396 pairs and 44 allocation rows; the
[repeat audit](https://github.com/mingley/pure-protobuf/actions/runs/42782528800)
recomputed 240 pairs and 20 allocation rows. Original artifact ZIP SHA-256
values are
`cd45631b4fc959aedd042e4b3f00b838f57b2d1c6968efeae0b3653917d4a00b`
and
`619391886bfa90aaebd00de22d68f2e8ee18cfe6e0c6a343ca920d66d157eec1`,
respectively. Both expire February 8, 2027.

While those artifacts remain available, rerun the arithmetic checks:

```sh
(
  set -eu
  repo_dir="$PWD"
  temp_dir="$(mktemp -d)"
  trap 'rm -rf "$temp_dir"' EXIT
  gh run download 42776117586 --repo mingley/pure-protobuf \
    --name codegen-literal-holdout-20261110 --dir "$temp_dir/holdout"
  python3 "$repo_dir/docs/benchmarks/compact-descriptor-literals-holdout-verify.py" "$temp_dir/holdout"
  cmp "$temp_dir/holdout/verified-holdout.json" \
    "$repo_dir/docs/benchmarks/compact-descriptor-literals-holdout-20261110.json"
  gh run download 42780774644 --repo mingley/pure-protobuf \
    --name codegen-literal-lean-repeat-20261110 --dir "$temp_dir/repeat"
  python3 "$repo_dir/docs/benchmarks/compact-descriptor-literals-repeat-verify.py" "$temp_dir/repeat"
  cmp "$temp_dir/repeat/verified-holdout.json" \
    "$repo_dir/docs/benchmarks/compact-descriptor-literals-repeat-20261110.json"
)
```

The independently checked [consumer samples](compact-descriptor-literals-consumer-20261110.json)
and [resource summary](compact-descriptor-literals-consumer-summary-20261110.json)
contain all 30 pairs and controls, source manifests, RSS samples and release
binary sizes. The [consumer audit](https://github.com/mingley/pure-protobuf/actions/runs/42786385340)
recomputed the ratios, checked the five retained source pairs and rejected a
mutated timing. Its [90-day bundle](https://github.com/mingley/pure-protobuf/actions/runs/42786385340/artifacts/13749197706)
has SHA-256
`181b84a2c05198151e186e15477ffd660a2e063c03b4e2e1a2904cbbb2969797`
and expires February 8, 2027. To check those samples and generated files:

```sh
(
  set -eu
  repo_dir="$PWD"
  temp_dir="$(mktemp -d)"
  trap 'rm -rf "$temp_dir"' EXIT
  gh run download 42786385340 --repo mingley/pure-protobuf \
    --name verified-literal-consumer-20261110 --dir "$temp_dir/bundle"
  unzip -q "$temp_dir/bundle/consumer.zip" -d "$temp_dir/consumer"
  unzip -q "$temp_dir/bundle/fixture.zip" -d "$temp_dir/fixture"
  python3 "$repo_dir/docs/evidence/compact-descriptor-literals-20261110/verify-generation-source.py" "$temp_dir/fixture"
  python3 "$repo_dir/docs/benchmarks/compact-descriptor-literals-consumer-verify.py" "$temp_dir/consumer"
  python3 "$repo_dir/docs/benchmarks/compact-descriptor-literals-consumer-sources-verify.py" "$temp_dir/consumer" "$temp_dir/fixture"
  cmp "$temp_dir/consumer/verified-consumer.json" \
    "$repo_dir/docs/benchmarks/compact-descriptor-literals-consumer-20261110.json"
  cmp "$temp_dir/consumer/verified-resource-summary.json" \
    "$repo_dir/docs/benchmarks/compact-descriptor-literals-consumer-summary-20261110.json"
)
```
