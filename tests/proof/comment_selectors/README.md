# Selective source comments

`Config::disable_comments_for(paths)` replaces the list of protobuf selectors.
`.` matches all names, qualified paths match prefixes, and relative paths
match suffixes on whole components. The existing `disable_comments(true)`
remains a global override; false restores selected behavior. An empty selector
list resets selection. Explicit user attributes and synthetic API docs remain.

Cache dependencies, then run from the repository root:

```sh
cargo fetch --locked --manifest-path tests/proof/comment_selectors/Cargo.toml
python3 tests/proof/comment_selectors/run.py --out work/comment-selectors
```

Fifteen compiled native/Prost 0.14.4 consumers cover packages, imported/nested
messages, fields, enums/values, suffixes, boundary nonmatches, reset, global
controls and preserved attributes. Both generators use the same source-info
FDS. Each registry is included at crate root, as imported generated paths
require. Consumers exchange wire values and mutate an imported child.

The build also checks builder/plugin/FDS identity, sequential and parallel
isolation, and selected RPC method-comment routing in three stub modes. RPC
checks are generated-source checks, not compiled transport consumers.
The plugin accepts repeated `disable_comments_for=path` entries; the hex form
preserves selectors containing commas or whitespace. Missing values and
malformed encodings are rejected.

Use `--baseline-plugin /path/to/protoc-gen-pbrs` for an ordinary-output byte
golden against a frozen previous generator. `--target-dir` accepts a dedicated
cache. Source hashes, compiler output, generated files and exact named consumer
results are retained. This proof does not establish every possible schema or
measure performance.
