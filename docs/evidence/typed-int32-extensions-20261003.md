# Selected native int32 extensions

The generator now accepts repeatable `typed_extension=FQN` selections and the
matching `Config` builder option. Supported same-file Edition 2024 singular
`int32` extensions produce host-typed identifiers backed by cold unknown-field
storage. Getters use the last matching varint or the descriptor default without
allocating. Set and clear preserve unrelated and same-number wrong-wire fields,
retain storage capacity, and invalidate cached size.

Unsupported selections and namespace collisions fail before output. The
wrong-host consumer fails to compile. Selecting no extensions preserves the
existing generated output and parse policy. Broader scalar kinds, repeated,
enum, message, cross-file and view accessors, and the official upb registry/ABI
remain separate open work.

## Ordinary qualification

The composed source is `71635367928994b1df46378f5dc7cdfe32b3f219`, based on
`68e4d57d67c73ff87ea1404bee18c3c70a9dfb93`. Its 19 commits integrate without
conflicts on main. All 11 changed files match the qualified source byte for byte.
The combined-main tree at `c7f21cd8c79127501ecd088f5fc02e565706f55a`
was then qualified in an isolated worktree with the identical Git tree. Rust
1.88 passed the 4 primitives, all 16 generator cases, the actual build-script
fallback test and strict core Clippy. Both consumers produced new complete
locked metadata graphs: the only change is the new std-only
`build_script_fallback` local test target. All 14 registry tuples, dependency
edges, dependency kinds, features and provider edges remain identical. The
independent full readback is
`492903b1e455320780bbb588709d014a123d90ddd0cfa8584b692247955e5e98`.
This combined-main check uses the fallback build context; it does not qualify
a separate SDK context or the gRPC runtime.

| Check | Actual result |
|---|---|
| Rust 1.88 primitives and selected generator cases | 4 and 16 passed |
| Rust 1.88 primary and renamed-runtime consumers | 7 and 1 passed; wrong-host rejection reproduced |
| Rust 1.88 strict Clippy and Rustdoc | Core and both consumers passed; Rustdoc passed |
| Affected decoding tests | 213 parent cases plus all 9 generated group and 15 generated map cases passed |
| Rust 1.85 | Both core feature configurations, 4 primitives, 7 primary and 1 alias cases passed; wrong-host rejection reproduced |
| Generation and dependency controls | All 6 raw controls and all 13 genuine regeneration modules remained byte-identical; accepted locks, graphs and feature-off graph preserved |
| Formatting | Scoped formatting and whitespace checks passed |

Quiet child case names are inferred from retained source and complete runs with
zero ignored, measured or filtered cases. Requested child command strings are
retained; they are not an independent child process trace.

The initial fresh build retained 31 selected files, including four executables,
with 101,145,061 bytes of full payload. New executed binaries and child payloads
were also retained and read back. The final qualification summary is
`23315835d21dafac1b9bf4a550ee271281d08ad1bbac155f929b6b3cc63350f4`.
These payloads and raw logs remain in the execution workspace; this page does
not bundle them.

Failed Python interpretations remain recorded. An initial Rust 1.85 primary
consumer attempt was interrupted at the fixed shared-cache limit. Its partial
result remains unqualified; only that interrupted check was retried after
resource recovery, while the three earlier successes retained their original
hashes and timestamps. Commands used offline locked dependencies, one build
job, zero incremental/debug settings and the original 900-second deadline.
The final continuation monitored eleven shared target roots against the
unchanged 2 GiB cache ceiling and free-space floor.

This qualifies the stated correctness and compatibility scopes. It supplies no
comparative performance result or fastest-implementation claim.
