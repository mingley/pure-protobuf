# Status trailer allocations, November 9

A focused allocation check compared formatting the numeric gRPC status with using static
header values. The candidate removes two allocations per call in the measured non-bare
cases. This is a construction diagnostic, not an RPC latency measurement or a comparison
with tonic.

| Status trailer | Allocations before | Allocations after | Requested bytes before | Requested bytes after |
|---|---:|---:|---:|---:|
| Bare OK | 2 | 2 | 108 | 108 |
| OK with metadata | 4 | 2 | 665 | 656 |
| CANCELLED | 4 | 2 | 665 | 656 |
| UNAVAILABLE | 4 | 2 | 666 | 656 |
| UNAVAILABLE with message, metadata, details and pushback | 9 | 7 | 742 | 732 |

The bare-OK row is an unchanged control. These counts are for standalone trailer maps;
they do not measure a server returning maps to its transport pool. Allocation totals
include successful allocation, zeroed allocation and reallocation calls. Requested
bytes include the full requested size of a reallocation and are not resident memory. The
rich-status row had one reallocation on each side; the other rows had none.

The check used Rust 1.85.0 and LLVM 19.1.7 on a hosted Linux x86-64 runner. Each case
was warmed up, then measured in separate test processes at 10,000 and 20,000 calls, with
two identical samples per size and ABBA ordering. The table shows the per-call
difference between the two sizes. A process-wide counting allocator was enabled only
during trailer construction and drop; each result was consumed, and representative wire
values were checked outside the measured loop.

The baseline runtime file came from
[84f84896](https://github.com/mingley/pure-protobuf/commit/84f84896fd6f2f1ff96210101834dff835a0f503).
The candidate runtime file and identical test fixture came from isolated preflight
commit
[d57158fe](https://github.com/mingley/pure-protobuf/commit/d57158fe75ea9a7ab56e900044d35c5130e78cbd).
The same checkout and probe were used for both builds, swapping only the runtime file.
The [workflow run](https://github.com/mingley/pure-protobuf/actions/runs/42669768089)
contains Rust 1.85 wire and Web controls, strict Clippy, and the allocation check.
All five listed jobs passed, but GitHub marked the workflow as failed without listing
a failed job; its cleanup job did not run. The available records do not explain why.

The [raw artifact](https://github.com/mingley/pure-protobuf/actions/runs/42669768089/artifacts/13699471154)
is retained until February 7, 2027 (SHA-256
`1e0eafae011e9e1c3cdfc009f5b83445c58e5659d6078cdf03d6f3a9bd4cec04`).

CPU time, latency, throughput and retired instructions were not measured. The result
does not establish an end-to-end improvement or an overall ranking.
