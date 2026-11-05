# Codegen naming costs, November 5

This diagnostic compares `1c6ec8ca` with the final core snapshot used for the
enum and type-name collision fixes. The candidate is the pinned baseline plus
seven archived core-file overlays, including the runtime changes. The fixes
make selected valid schemas compile; these measurements establish neither a
generator speedup nor universal nonregression.

The retained CG19 CLI generates 6, 100 and 1,000 messages in 2, 5 and 20 files.
Fresh generation starts with an empty output directory. Unchanged generation
repeats the same command against its existing files. Neither phase flushes OS
or filesystem caches. Timing includes protoc, generator work, output handling
and process overhead. Output hashing and RSS-sampler teardown occur outside
the command timing boundary. Consumer builds are not measured.

Both CLI binaries were freshly built with Rust 1.99.1, LLVM 23.1.1,
optimization level 3, thin LTO and one codegen unit. Their dependency enables
only `codegen`; the generator still emits its normal reflection, JSON and
text helpers. Each build records 70 selected core inputs, the CLI source,
dependency lockfile, compiler version and executable hash. The manifests use
a root-only workspace. These are source and build records, not a filesystem
read trace or hosted attestation.

Five randomized pairs produced these new/old CLI wall-time ratios:

| Messages | Phase | Median | Pair range |
|---|---|---:|---:|
| 6 | Fresh | 1.013 | 0.922–1.177 |
| 6 | Unchanged | 0.993 | 0.894–1.112 |
| 100 | Fresh | 0.999 | 0.947–1.033 |
| 100 | Unchanged | 1.044 | 0.971–1.231 |
| 1,000 | Fresh | 1.000 | 0.870–1.119 |
| 1,000 | Unchanged | 1.013 | 0.952–1.100 |

All 120 processes succeeded. The normal-schema output trees match between
old and new, and unchanged-generation mtimes are preserved. The retained
Prost binary served as an optional fresh noise control. Its binary, manifest,
lockfile and CLI source match earlier build metadata, but that metadata lacks
a compiler version; this packet does not establish a Prost ranking.

A focused nine-pair holdout added 36 successful processes for the 100-message
case. Fresh generation has a median ratio of 1.001, range 0.936–1.104.
Unchanged generation has a median of 1.037, range 0.932–1.151; six of nine
ratios are above one. The positive unchanged-generation overhead persists in
the medians, though shared-host variation remains wide. No production code
was changed after these measurements. The allocator still constructs some
companion-name strings during both checking and insertion; reusing them is a
candidate for a measured follow-up.

Run the archive and arithmetic checks from the repository root:

```sh
python3 docs/evidence/codegen-naming-20261105/check.py
```

The checker verifies every archived file, reconstructs the selected baseline
and candidate sources from Git and the overlays, checks their build-record
hashes, and recomputes all initial and focused timing ratios. It also checks
the internal consistency of the observed output hashes. Executables and
generated output trees are omitted: independent verification of those outputs
requires rebuilding and regenerating. The checker cannot certify host
isolation or the honesty of recorded observations.

The [manifest](manifest.json) retains every row and source limit.
[Raw records](raw.tar.gz) contain 428 files: all 156 timed process records and
logs, synthetic proto corpora, source overlays, build manifests and controllers.
The build and timing scripts record their original workspace paths; adjust
those paths when reconstructing the archived source in another checkout.
