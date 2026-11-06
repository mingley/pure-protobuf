# Safe message default costs, November 5

`Config::safe_message_defaults(true)` lets generated bindings compile in a
consumer with `#![forbid(unsafe_code)]`. It derives field defaults instead of
calling the generated zero-initialization helper. This is an opt-in policy
feature, disabled by default. Runtime dependencies retain their own unsafe
code. These observations support no speed claim or broad nonregression claim.

The actual generated consumer showed an approximately **20% release
empty-encode regression**: safe/ordinary median 1.201, with all five pairs
between 1.180 and 1.203. Debug construction was also slower. Parse and
read-all results do not establish a general performance difference.

## Actual generated consumer

Both variants use the same proto2/proto3 fixtures, generator source and
runtime. The ordinary control enables the proof crate's `ordinary-control`
feature; the other consumer forbids unsafe code and enables safe defaults.
This timing screen uses lazy message storage, no RPC stubs and no reflection.
It does not compare against Prost or tonic.

Rust 1.99.1 and LLVM 23.1.1 built both variants. Dev uses optimization level 0
with debug information disabled. Release uses Cargo's release defaults,
thin LTO and one codegen unit. Incremental compilation is disabled. The
controller pins its processes to CPU 0, randomizes case and variant order,
and runs five pairs for each row: 80 successful processes in total.

Times come from `Instant` inside the consumer. They exclude process startup
and preparation of the populated input. `default` constructs, exposes and
drops a message; `empty_encode` constructs and serializes an empty message.
`parse` parses the populated input, and `read_all` parses it and checks every
populated field and payload byte. Every process records the same input bytes,
expected checksum and message sizes. This was a short shared-host screen;
coordinating other builds did not establish host isolation.

Ratios are safe/ordinary; values above one mean more elapsed time.

| Profile | Operation | Median | Pair range |
|---|---|---:|---:|
| Dev | Default | 1.395 | 1.393–1.438 |
| Dev | Empty encode | 1.134 | 1.119–1.213 |
| Dev | Parse | 1.030 | 1.004–1.046 |
| Dev | Parse and read all | 1.028 | 1.023–1.034 |
| Release | Default | 0.875 | 0.870–0.895 |
| Release | Empty encode | 1.201 | 1.180–1.203 |
| Release | Parse | 0.998 | 0.914–1.133 |
| Release | Parse and read all | 1.017 | 0.959–1.033 |

The smaller release construction time does not offset the empty-encode
regression or justify changing the default profile. Debug overhead is visible
here; its magnitude depends on the consumer and build configuration. Release
parse varies widely between pairs, and read-all measurements remain close
enough to require broader measurements before drawing a conclusion.

## Consumer build and output costs

Three additional randomized pairs per profile clean only the proof package's
artifacts, then rebuild its generated bindings, executable and build script.
All 12 builds pass. Cargo's artifact records confirm that dependencies stay
fresh. These are dependency-warm consumer builds, including code generation,
compilation and linking; they are not full clean workspace builds.

| Profile | Median safe/ordinary | Pair range |
|---|---:|---:|
| Dev | 1.036 | 0.992–1.046 |
| Release | 0.996 | 0.979–1.031 |

These few local observations establish neither a build speedup nor general
build-cost parity. The warm-build capture hashes the proof inputs and records
Cargo's dependency metadata; it does not take a separate complete core-source
or executable hash snapshot for each pair. Initial binary-freezing build
times have unequal cache conditions and are excluded from the comparison.

Generated Rust totals 127,480 bytes for ordinary defaults and 126,617 bytes
for safe defaults, including `mod.rs`: 863 fewer bytes. The measured dev
executable grows from 1,899,840 to 1,905,296 bytes; release grows from 808,952
to 809,704 bytes. These are whole executable file sizes for this fixture,
not library text sizes or a prediction for other applications. Message sizes
match: `Record` is 216 bytes and `Presence` is 224 bytes.

## Separate synthetic rejection of a global change

Before adding the opt-in profile, a synthetic prototype modeled the runtime
storage families with zeroed and derived defaults. It is not generated
message parsing, a consumer build, or an RPC benchmark. Five randomized pairs
cover 11 construction/drop or immediate-cache-read cases at optimization
levels 0 and 3, totaling 220 successful processes.

At optimization level 3, derived/zeroed construction ratios include:

| Synthetic case | Median | Pair range |
|---|---:|---:|
| Optional fields | 1.702 | 1.695–1.869 |
| Owned child storage | 1.346 | 1.336–1.457 |
| Optional fields, immediate cache read | 1.622 | 1.586–1.703 |
| Owned child storage, immediate cache read | 1.297 | 1.244–1.338 |

Field semantics, drop checks and layouts pass in all four synthetic builds.
The regressions were sufficient to reject blanket adoption. The full 22 rows
remain in [results.json](results.json), separate from actual consumer results.

## Check and reproduce

Run the archive, selected-source and arithmetic checks from the repository:

```sh
python3 docs/evidence/safe-defaults-20261105/check.py
```

To reconstruct the selected source inputs and unpack the original records:

```sh
python3 docs/evidence/safe-defaults-20261105/check.py \
  --reconstruct work/safe-defaults-reconstructed
```

The new directory contains `actual/` with the measured core and proof source,
`synthetic/runtime/` with the separate adapted runtime snapshot, and
`records/` with the original build/timing controllers and logs. Source
reconstruction uses Git commit `0ae8a43d3d08617c249c84a3f0d30f614494d70e`
plus archived patches. The actual snapshot includes other concurrent codegen
fixes, shared by both controls. The synthetic snapshot changes only its
workspace manifest and prunes workspace-only lockfile entries.

To build the actual snapshot, overlay `actual/` onto a complete checkout of
the pinned commit. The selected inventory omits unrelated workspace members,
so `actual/` alone is not a complete Cargo workspace. The adapted synthetic
runtime uses a separate root-only workspace.

The original controllers retain absolute workspace paths. Adjust their
`ROOT`, `HERE`, target and output paths for the reconstructed sources and use
fresh output directories before rerunning. The actual proof's Cargo manifest
and build script contain the exact feature and generation choices. Its
[README](../../../tests/proof/safe_defaults/README.md) describes the separate
functional controls for lazy, owned and selected boxed storage.

The [manifest](manifest.json) hashes every archived file. The 696-file
[archive](raw.tar.gz) contains raw observations, build records, harnesses and
controllers, with no executables, compiled libraries or full source trees.
The checker reconstructs selected inputs and recomputes all ratios. Build
records retain executable and generated-output hashes; those outputs must be
rebuilt to verify them independently. The timing controller references the
frozen build report, without rehashing binaries at every invocation. This is
local evidence, not execution attestation or proof of host isolation.

See the [codegen guide](../../guides/codegen.md) for the option's API and limits.
