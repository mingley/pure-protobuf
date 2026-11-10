# TLS portability x86-64 benchmark source

[Read the results](../../benchmarks/tls-portability-x86-20261110.md).

These files retain the probes and orchestration for the source-pinned
configuration and warmed-RPC comparisons. They are benchmark fixtures, not
production binaries or a release qualification.

| File | Role |
|---|---|
| `config-probe.rs` | Built-in client/server config construction and drop |
| `config-compare.py` | Eight-pair construction screen |
| `client-ca-repeat.py` | Twelve-pair longer client-CA screen |
| `config-workflow.yml`, `client-ca-workflow.yml` | Original source, build and runner settings |
| `config-Cargo.lock` | Resolved construction-probe dependencies |
| `rpc-probe.rs`, `rpc-compare.py` | Warmed unary fixture and paired controls |
| `rpc-workflow.yml`, `rpc-Cargo.lock` | Original RPC build and resolved dependencies |

The workflows used temporary paths and scratch branches. Both source
revisions and the public test certificates are identified in the result
record. Raw archived runs contain the active x86 feature trees and environment
details. Rust's counting allocator reports successful requested allocation
and reallocation sizes; it does not report peak memory.
