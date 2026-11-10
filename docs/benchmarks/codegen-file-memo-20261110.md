# Codegen filename memo experiments, November 10

[Benchmarks](../benchmarks.md)

Two scratch-only changes reduced temporary filename allocations in codegen.
The borrowed-key version showed an observed one-worker service/extern CPU
regression in two screens, so neither candidate was adopted. The measurements
are pbrs-to-pbrs comparisons and do not establish a result against Prost.

At baseline `5fe70efdc798031badefd1a3847d8866dde352f5`, codegen inserts each
message, enum and service filename into a local `BTreeMap<String, FileFacts>`
using `file.to_string()`, including repeated keys. Borrowed-key candidate
`9717d306baf1e0391153d8157e5c31990f4a3c92` changes that map to
`BTreeMap<&str, FileFacts>`, uses `.entry(file)`, and adjusts the private
lookup signature. Contains-key candidate
`5bbdac8f28fea40bd6330067f3b205eaa63db728` keeps owned keys, first checks
`contains_key(file)`, then allocates and inserts only on a miss. Both are
direct children of the baseline and only change
`src/codegen/descriptors.rs`.

## Method

The [initial screen](https://github.com/mingley/pure-protobuf/actions/runs/42774965420)
built all three versions with Rust 1.99.1, release optimization, thin LTO and
one codegen unit on a shared hosted AMD EPYC 7763. It measured six rotated
blocks per case and separate same-binary pairs. The
[longer repeat](https://github.com/mingley/pure-protobuf/actions/runs/42776739174)
compared only baseline and borrowed-key with 12 alternating-order blocks on
a shared hosted AMD EPYC 9V45. Both pinned one or two guest-distinct cores and
checked the process's reported parallelism; physical host isolation was not
guaranteed.

The probe repeatedly calls codegen with a prepared, in-memory
`CodeGeneratorRequest`. It excludes `protoc`, process startup, request
file I/O, generated-file writes and Rust compilation. The process CPU clock
includes work by codegen worker threads and output destruction. Both screens
use the same 13 request hashes and regenerate outputs separately. Their
separate allocation binary counts successful allocation, zeroed-allocation
and reallocation requests and requested sizes, after two warmups, over two
operations. Requested bytes are cumulative, including the new size on
reallocation; they are not peak live heap or RSS.

The balanced case has 1,000 messages across 20 files; default and lean options
are separate. The many cases have 128 or 512 files, each with a two-field
message. Alias128 targets basenames for 128 nested files. The service case has
two target files, two services and an imported external type; a source marker
checks that a client stub and the external mapping were emitted. Lean means
`no_reflect=true,emit_json=false,emit_text=false`.

At one and two workers, the initial screen compared returned file names and
actual bytes across all three versions for successful cases, and compared
errors for ambiguous and unknown targets. The duplicate-descriptor case
succeeded and its outputs were compared too. The repeat compared
actual bytes for baseline and borrowed-key again and retained separate
per-output name, length and SHA-256 manifests for both. This is evidence for
these cases, not complete semantic, concurrency or MSRV coverage. The
independent repeat verifier compares retained manifests; generated output
bytes themselves were discarded after the collection-time comparison.

## CPU results

Each candidate/base entry is median (minimum–maximum; number of slower
candidate blocks). Control columns are the range of the second same-binary
sample divided by the first; they show variability but are not confidence
intervals.

Initial service and unique-file holdouts, six blocks:

| Case | Workers | Borrowed / base | Contains / base | Base control | Borrowed control | Contains control |
|---|---:|---:|---:|---:|---:|---:|
| services | 1 | 1.0267 (1.0002–1.0796; 6) | 1.0241 (1.0009–1.0394; 6) | 0.9654–1.0219 | 0.9841–1.0210 | 0.8291–1.0059 |
| services | 2 | 1.0088 (0.9615–1.0232; 4) | 1.0031 (0.9827–1.0274; 4) | 1.0017–1.0235 | 0.8967–1.0167 | 0.9722–1.1078 |
| many128 | 1 | 0.9777 (0.9736–0.9823; 0) | 1.0087 (0.9998–1.0377; 5) | 0.9900–1.0741 | 0.9915–1.0219 | 1.0005–1.0163 |
| many128 | 2 | 0.9807 (0.9702–0.9947; 0) | 1.0121 (0.9945–1.0355; 5) | 0.9820–1.0283 | 0.9837–1.0412 | 0.9687–1.0297 |
| many512 | 1 | 0.9685 (0.9636–0.9822; 0) | 1.0085 (1.0027–1.0169; 6) | 0.9879–1.0129 | 0.9936–1.0092 | 0.9966–1.0080 |
| many512 | 2 | 0.9645 (0.9579–0.9823; 0) | 1.0083 (0.9993–1.1038; 4) | 0.9728–1.0862 | 0.9887–1.0186 | 0.9935–1.0107 |

The initial service samples were short: approximately 5.4–11.0 ms of
process CPU per sample. The longer repeat aimed for 300 ms using a pilot and
allowed up to 20,000 iterations; the one-worker service samples measured
235–259 ms. Its results follow:

| Case | Workers | Borrowed / base | Base control | Borrowed control |
|---|---:|---:|---:|---:|
| services-lean | 1 | 1.0443 (1.0150–1.0854; 12) | 0.9768–1.0409 | 0.9793–1.0858 |
| services-lean | 2 | 1.0024 (0.8397–1.0422; 6) | 0.8464–1.0320 | 0.8403–1.0742 |
| many128-lean | 1 | 0.9607 (0.9137–1.1668; 2) | 0.8734–1.0320 | 0.8755–1.0842 |
| many128-lean | 2 | 0.9858 (0.8534–1.0384; 2) | 0.9411–1.3780 | 0.9053–1.2777 |
| many512-lean | 1 | 0.9894 (0.8794–1.1096; 5) | 0.9493–1.0930 | 0.8926–1.0303 |
| many512-lean | 2 | 0.9899 (0.9684–1.2488; 4) | 0.9613–1.0642 | 0.9537–1.1139 |
| balanced20-default | 1 | 0.9967 (0.9635–1.0822; 5) | 0.8517–1.0784 | 0.9064–1.1814 |
| balanced20-default | 2 | 1.0067 (0.9181–1.0226; 7) | 0.9634–1.0864 | 0.8817–1.2464 |
| balanced20-lean | 1 | 1.0005 (0.8603–1.0518; 6) | 0.9576–1.0883 | 0.8916–1.0829 |
| balanced20-lean | 2 | 1.0073 (0.9729–1.0349; 8) | 0.9387–1.0508 | 0.9676–1.0391 |
| alias128-lean | 1 | 0.9755 (0.8818–1.0976; 3) | 0.9496–1.0733 | 0.9082–1.1278 |
| alias128-lean | 2 | 1.0016 (0.9474–1.0273; 7) | 0.8733–1.0253 | 0.9742–1.0266 |

The one-worker service median was 1.0443 in the repeat, with all 12 paired
blocks slower; the earlier six blocks were also slower. Same-binary control
ranges overlap the observed difference, and the hosts were shared, so the
measurements do not identify the cause. The two-worker service result and
other cases are mixed. The first screen favored borrowed keys on the unique
files, but the repeat had wide ranges. This is insufficient evidence for a
general CPU improvement.

## Allocation requests

For one worker, entries are successful requests / cumulative requested bytes
per codegen operation. These counts match across the three repeat samples for
baseline and borrowed-key; contains-key was measured in the initial screen.

| Case | Baseline | Borrowed | Contains |
|---|---:|---:|---:|
| balanced20-default | 290,689 / 123,605,748 | 289,213 / 123,586,208 | 289,233 / 123,586,820 |
| balanced20-lean | 167,960 / 47,064,477 | 166,484 / 47,044,937 | 166,504 / 47,045,549 |
| many128-lean | 53,424 / 15,885,026 | 53,296 / 15,881,642 | 53,424 / 15,885,026 |
| many512-lean | 607,715 / 213,413,818 | 607,203 / 213,400,282 | 607,715 / 213,413,818 |
| alias128-lean | 54,320 / 17,109,337 | 54,192 / 17,104,801 | 54,320 / 17,109,337 |
| services-lean | 575 / 477,221 | 569 / 477,068 | 572 / 477,191 |

The borrowed version saved 1,476 requests on balanced20 and 128 or 512 on
the unique-file cases. Contains-key saved allocations for repeated filenames
but none on those unique-file cases. These savings did not settle the CPU
tradeoff.

## Evidence

[Extracted timing blocks and allocation summaries](codegen-file-memo-20261110.json)
retain all 72 initial and 144 repeat timing blocks, including controls; run
`python3 docs/benchmarks/codegen-file-memo-20261110-verify.py` from the
repository root to recompute their summaries. The data are extracted from the
run and independent-verification logs; request bytes and full output manifests
are in the artifacts below. The [probe and collection sources](../evidence/codegen-file-memo-20261110/)
and source-pinned workflow files preserve the original setup and commands.

The [initial verification](https://github.com/mingley/pure-protobuf/actions/runs/42776241611)
checked its archive and measurements. The
[repeat verification](https://github.com/mingley/pure-protobuf/actions/runs/42777875828)
checked both archive digests, request hashes, per-variant manifests, source
pins, 144 timing blocks and 72 allocation samples. Its
[combined artifact](https://github.com/mingley/pure-protobuf/actions/runs/42777875828/artifacts/13744970848)
expires February 8, 2027 at 07:48:30 UTC; ZIP SHA-256:
`e100dc2f31ba11cf3b02cefde7823b5bb284beae4bb9af24a9c91a8f818747bc`.
It contains the two original archives and their extracted data. The initial
and repeat source ZIP SHA-256 values are respectively
`3ec488759acd5e27f234c222c6b64f6aa97ae162ea6ff3d3a39cba3ee7c01a7e` and
`37eca01f45d096eb45924c9164f53038127c2e1a4c1bb56d344d8438db31ff64`.
