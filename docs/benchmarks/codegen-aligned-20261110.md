# Aligned codegen settings, November 10

[Benchmarks](../benchmarks.md)

With source-location and formatter settings aligned, the reflection-free pbrs
driver used more process CPU than Prost in all eight pairs for each of three
synthetic schemas. This is a scoped generation comparison: pbrs and Prost
still generate different APIs and amounts of code.

The [screen](https://github.com/mingley/pure-protobuf/actions/runs/42770907500)
compared pbrs at
[`5fe70efd`](https://github.com/mingley/pure-protobuf/commit/5fe70efdc798031badefd1a3847d8866dde352f5)
with Prost 0.14.4. Pbrs lean disabled reflection, JSON and text; Prost aligned
disabled source-location information and its default formatting pass. Both
pbrs configurations also disabled source-location information. The default
Prost configuration retained both source locations and formatting.

| Schema | pbrs default / Prost default CPU: median (range) | pbrs lean / Prost aligned CPU: median (range) | Prost aligned / default CPU: median (range) |
|---|---:|---:|---:|
| 50 messages, 1 file | 1.0790 (1.0475–1.1762) | 1.4621 (1.4225–1.4774) | 0.6442 (0.6293–0.6688) |
| 100 messages, 5 files | 1.1054 (1.0830–1.1261) | 1.6999 (1.6555–1.7132) | 0.5457 (0.5385–0.5626) |
| 1,000 messages, 20 files | 1.1374 (1.1251–1.1717) | 2.1100 (2.0775–2.1455) | 0.4033 (0.3968–0.4114) |

Pbrs was slower in all eight pairs for both pbrs/Prost columns. The
Prost aligned/default column changes two settings together; it does not
attribute their effect individually. For the three schemas, independently
retained descriptor sets for pbrs default, pbrs lean and Prost aligned were
byte-for-byte equal. Prost default's descriptor set differed because it
included source locations.

| Schema | pbrs default Rust bytes | pbrs lean Rust bytes | Prost aligned Rust bytes |
|---|---:|---:|---:|
| 50 messages, 1 file | 1,038,730 | 607,264 | 19,817 |
| 100 messages, 5 files | 2,491,030 | 1,312,998 | 41,797 |
| 1,000 messages, 20 files | 34,347,839 | 12,925,901 | 412,622 |

These are different generated feature surfaces and layouts, not equivalent
outputs. The 1,000-message pbrs output contained 20 files with 50 distinct
message structs each and a registry file: 1,000 distinct structs, with no
duplicate message definitions. Its lean output contained no
`FILE_DESCRIPTOR_SET` occurrence; default output contained 20 descriptor
literals. The lean path emits no descriptor literal, although whole-binary
code-layout effects remain possible.

## What the generated source contains

A [separate replay](https://github.com/mingley/pure-protobuf/actions/runs/42779103755)
retained the 1,000-message lean output and approximately grouped its 12,925,901
source bytes by lexical section. It attributed 4,147,341 bytes to merge and
parse sections, 2,570,888 to validation, 2,018,592 to size and encoding, and
1,200,862 to accessors. The remaining bytes include comments and other code.
These are approximate source-text categories, not AST sizes or CPU samples.

The opt-in [shared descriptor setting](../guides/codegen.md#shared-descriptor-storage)
places reflective multi-file metadata in one helper, with a corresponding
crate-root module and file-layout requirement. It is off by default and was
not measured in this comparison. Reflection-free output is unchanged by that
setting.

The replay's [recorded breakdown](codegen-aligned-source-20261110.json) and
[verifier](codegen-aligned-source-20261110-verify.py) are kept with this note.
Its [90-day bundle](https://github.com/mingley/pure-protobuf/actions/runs/42779103755/artifacts/13745012286)
has SHA-256
`fa591e3ede48126599a8216a852718fbe2b774bbdc042a5f9375a22a23a6d908`
and expires February 8, 2027. To check the original source archive and
recompute the categories:

```sh
(
  set -eu
  repo_dir="$PWD"
  temp_dir="$(mktemp -d)"
  trap 'rm -rf "$temp_dir"' EXIT
  gh run download 42779103755 --repo mingley/pure-protobuf \
    --name verified-aligned-codegen-sections-5fe --dir "$temp_dir"
  printf '%s  %s\n' \
    b0f8bb25b256bfefa1c264fadb66b5f1dbcd11da585553ae9e2da3738cf42dfb \
    "$temp_dir/source.zip" | sha256sum --check
  python3 "$repo_dir/docs/benchmarks/codegen-aligned-source-20261110-verify.py" \
    "$temp_dir/source" "$repo_dir/docs/benchmarks/codegen-aligned-source-20261110.json"
)
```

## Method

Both drivers were optimized Rust 1.99.1 builds. Each timed invocation included
driver startup, protoc 3.21.12, generation and writes to a fresh directory; it
excluded compilation of the generated Rust. The single-package inputs had
50, 100 and 1,000 messages, with 1, 5 and 20 files respectively. Each of eight
blocks rotated and reversed the four variants; a block summed process CPU over
12, 7 or 1 invocations per variant, respectively. Each same-binary control
is its second CPU sample divided by its first.

| Schema | pbrs default control range | pbrs lean control range | Prost default control range | Prost aligned control range |
|---|---:|---:|---:|---:|
| 50 messages, 1 file | 0.9923–1.1118 | 0.9692–1.0056 | 0.8988–1.0068 | 0.9926–1.0067 |
| 100 messages, 5 files | 0.9938–1.0015 | 0.9940–1.0045 | 0.9886–1.0188 | 0.9956–1.0172 |
| 1,000 messages, 20 files | 0.9930–1.0147 | 0.9687–1.0175 | 0.9823–1.0116 | 0.9602–0.9953 |

The runs used one pinned logical CPU on a shared AMD EPYC 9V74 runner. The
ranges and control values limit the conclusions; this is not a full
feature-parity or overall performance ranking.

The [retained data](codegen-aligned-20261110.json) contains all 24 blocks,
individual invocation timings, controls, descriptor hashes and output
summaries. The [independent verifier](https://github.com/mingley/pure-protobuf/actions/runs/42771970891)
recomputed the timing ratios, compared the retained descriptor bytes directly,
and regenerated the exact 1,000-message output to count its types and
literals. That verification did not retain generated source; the later source replay above did. The
[90-day bundle](https://github.com/mingley/pure-protobuf/actions/runs/42771970891/artifacts/13742605866)
has SHA-256
`663dfacf4aa4ea3dda376c0ddce93bf1780da86477c9a2c6235c1cb0cbaf8ebc`
and expires February 8, 2027. From the repository root, the following checks
the original archive, descriptor bytes and timing data while the bundle
remains available:

```sh
(
  set -eu
  repo_dir="$PWD"
  temp_dir="$(mktemp -d)"
  trap 'rm -rf "$temp_dir"' EXIT
  cd "$temp_dir"
  gh run download 42771970891 --repo mingley/pure-protobuf \
    --name codegen-aligned-verified-20261110 --dir bundle
  python3 "$repo_dir/docs/benchmarks/codegen-aligned-20261110-verify.py" \
    bundle/original.zip "$repo_dir/docs/benchmarks/codegen-aligned-20261110.json"
)
```

The verification script checks the original ZIP SHA-256 before reading it.
It checks the recorded output-partition summary against the original
measurements; regenerating and inspecting the source requires rebuilding the
pinned driver as the independent verifier did.
